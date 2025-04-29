use crate::ast::{BinaryOp, Expr, Function, Prototype};
use crate::error::Error::*;
use crate::error::Result;

use cranelift::codegen::ir::InstBuilder;
use cranelift::codegen::settings::Configurable;
use cranelift::prelude::{
    AbiParam, EntityRef, FloatCC, FunctionBuilder, FunctionBuilderContext, Value, Variable, isa,
    settings, types,
};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

use std::collections::HashMap;
use std::mem;
use target_lexicon::triple;

pub struct Generator {
    builder_context: FunctionBuilderContext,
    functions: HashMap<String, CompiledFunction>,
    module: JITModule,
    variable_builder: VariableBuilder,
}

impl Generator {
    pub fn new() -> Self {
        let mut flag_builder = settings::builder();
        flag_builder
            .set("opt_level", "speed_and_size")
            .expect("set optlevel");
        let isa_builder = isa::lookup(triple!("x86_64-unknown-unknown-elf")).expect("isa");
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .expect("finish");
        let builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());

        Self {
            builder_context: FunctionBuilderContext::new(),
            functions: HashMap::new(),
            module: JITModule::new(builder),
            variable_builder: VariableBuilder::new(),
        }
    }

    pub fn prototype(&mut self, prototype: &Prototype, linkage: Linkage) -> Result<FuncId> {
        let function_name = &prototype.function_name;
        let parameters = &prototype.parameters;

        match self.functions.get(function_name) {
            None => {
                let mut signature = self.module.make_signature();
                for _parameter in parameters {
                    signature.params.push(AbiParam::new(types::F64));
                }
                signature.returns.push(AbiParam::new(types::F64));

                let id = self
                    .module
                    .declare_function(function_name, linkage, &signature)?;
                self.functions.insert(
                    function_name.to_string(),
                    CompiledFunction {
                        defined: false,
                        id,
                        param_count: parameters.len(),
                    },
                );

                Ok(id)
            }
            Some(function) => {
                if function.defined {
                    return Err(FunctionRedef);
                }
                if function.param_count != parameters.len() {
                    return Err(FunctionRedefWithDifferentParams);
                }
                Ok(function.id)
            }
        }
    }

    pub fn function(&mut self, function: Function) -> Result<fn() -> f64> {
        let mut context = self.module.make_context();
        let signature = &mut context.func.signature;
        let parameters = &function.prototype.parameters;
        for _parameter in parameters {
            signature.params.push(AbiParam::new(types::F64));
        }
        signature.returns.push(AbiParam::new(types::F64));

        let function_name = function.prototype.function_name.to_string();
        let func_id = self.prototype(&function.prototype, Linkage::Export)?;

        let mut builder = FunctionBuilder::new(&mut context.func, &mut self.builder_context);
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        let mut values = HashMap::new();
        for (i, name) in parameters.iter().enumerate() {
            let val = builder.block_params(entry_block)[i];
            let variable = self.variable_builder.create_var(&mut builder, val);
            values.insert(name.clone(), variable);
        }

        if let Some(ref mut function) = self.functions.get_mut(&function_name) {
            function.defined = true;
        }

        let mut generator = FunctionGenerator {
            builder,
            functions: &self.functions,
            module: &mut self.module,
            values,
        };
        let return_value = match generator.expr(function.body) {
            Ok(value) => value,
            Err(error) => {
                generator.builder.finalize();
                self.functions.remove(&function_name);
                return Err(error);
            }
        };
        generator.builder.ins().return_(&[return_value]);
        generator.builder.finalize();

        context.optimize(
            self.module.isa(),
            &mut cranelift_control::ControlPlane::default(),
        )?;

        println!("{}", context.func.display());

        self.module.define_function(func_id, &mut context)?;
        self.module.clear_context(&mut context);
        self.module.finalize_definitions()?;

        if function_name.starts_with("__anon_") {
            self.functions.remove(&function_name);
        }

        unsafe {
            Ok(mem::transmute::<*const u8, fn() -> f64>(
                self.module.get_finalized_function(func_id),
            ))
        }
    }
}

struct VariableBuilder {
    index: usize,
}

impl VariableBuilder {
    fn new() -> Self {
        Self { index: 0 }
    }

    fn create_var(&mut self, builder: &mut FunctionBuilder, value: Value) -> Variable {
        let variable = Variable::new(self.index);
        self.index += 1;

        builder.declare_var(variable, types::F64);
        builder.def_var(variable, value);

        variable
    }
}

struct CompiledFunction {
    defined: bool,
    id: FuncId,
    param_count: usize,
}

pub struct FunctionGenerator<'a> {
    builder: FunctionBuilder<'a>,
    functions: &'a HashMap<String, CompiledFunction>,
    module: &'a mut JITModule,
    values: HashMap<String, Variable>,
}

impl FunctionGenerator<'_> {
    fn expr(&mut self, expr: Expr) -> Result<Value> {
        let value = match expr {
            Expr::Number(num) => self.builder.ins().f64const(num),
            Expr::Variable(name) => match self.values.get(&name) {
                Some(&variable) => self.builder.use_var(variable),
                None => return Err(Undefined("variable")),
            },
            Expr::Binary(op, left, right) => {
                let left = self.expr(*left)?;
                let right = self.expr(*right)?;
                match op {
                    BinaryOp::Plus => self.builder.ins().fadd(left, right),
                    BinaryOp::Minus => self.builder.ins().fsub(left, right),
                    BinaryOp::Times => self.builder.ins().fmul(left, right),
                    BinaryOp::LessThan => {
                        let boolean = self.builder.ins().fcmp(FloatCC::LessThan, left, right);

                        self.builder.ins().fcvt_from_sint(types::F64, boolean)
                    }
                }
            }
            Expr::Call(name, args) => match self.functions.get(&name) {
                Some(func) => {
                    if func.param_count != args.len() {
                        return Err(WrongArgumentCount);
                    }
                    let local_func = self.module.declare_func_in_func(func.id, self.builder.func);
                    let arguments: Result<Vec<_>> =
                        args.into_iter().map(|arg| self.expr(arg)).collect();
                    let arguments = arguments?;
                    let call = self.builder.ins().call(local_func, &arguments);

                    self.builder.inst_results(call)[0]
                }
                None => return Err(Undefined("function")),
            },
        };

        Ok(value)
    }
}
