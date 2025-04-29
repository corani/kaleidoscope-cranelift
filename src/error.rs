use self::Error::*;
use cranelift::codegen::CodegenError;
use cranelift_module::ModuleError;
use std::fmt::{self, Debug, Formatter};
use std::io;
use std::num::ParseFloatError;
use std::result;

pub type Result<T> = result::Result<T, Error>;

pub enum Error {
    CraneliftModule(ModuleError),
    CraneliftCodegen(CodegenError),
    FunctionRedef,
    FunctionRedefWithDifferentParams,
    Io(io::Error),
    ParseFloat(ParseFloatError),
    UnknownChar(char),
    Undefined(&'static str),
    Unexpected(&'static str),
    WrongArgumentCount,
}

impl Debug for Error {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match *self {
            CraneliftModule(ref error) => error.fmt(formatter),
            CraneliftCodegen(ref error) => error.fmt(formatter),
            FunctionRedef => write!(formatter, "function redefinition"),
            FunctionRedefWithDifferentParams => {
                write!(formatter, "function redefinition with different parameters")
            }
            Io(ref error) => error.fmt(formatter),
            ParseFloat(ref error) => error.fmt(formatter),
            UnknownChar(char) => write!(formatter, "unknown char `{}`", char),
            Undefined(msg) => write!(formatter, "undefined {}", msg),
            Unexpected(msg) => write!(formatter, "unexpected {}", msg),
            WrongArgumentCount => write!(formatter, "wrong argument count"),
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Io(error)
    }
}

impl From<ParseFloatError> for Error {
    fn from(error: ParseFloatError) -> Self {
        ParseFloat(error)
    }
}

impl From<ModuleError> for Error {
    fn from(error: ModuleError) -> Self {
        CraneliftModule(error)
    }
}

impl From<CodegenError> for Error {
    fn from(error: CodegenError) -> Self {
        CraneliftCodegen(error)
    }
}
