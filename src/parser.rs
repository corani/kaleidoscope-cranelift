use crate::ast::{BinaryOp, Expr, Function, Prototype};
use crate::error::Error::{Undefined, Unexpected};
use crate::error::Result;
use crate::lexer::{Lexer, Token};

use std::collections::HashMap;
use std::io::Read;

pub struct Parser<R: Read> {
    pub lexer: Lexer<R>,
    bin_precedence: HashMap<BinaryOp, i32>,
    index: usize,
}

impl<R: Read> Parser<R> {
    pub fn new(lexer: Lexer<R>) -> Self {
        let mut bin_precedence = HashMap::new();
        bin_precedence.insert(BinaryOp::LessThan, 10);
        bin_precedence.insert(BinaryOp::Plus, 20);
        bin_precedence.insert(BinaryOp::Minus, 20);
        bin_precedence.insert(BinaryOp::Times, 40);

        Self {
            lexer,
            bin_precedence,
            index: 0,
        }
    }

    pub fn toplevel(&mut self) -> Result<Function> {
        let body = self.expr()?;
        self.index += 1;

        Ok(Function {
            body,
            prototype: Prototype {
                function_name: format!("__anon_{}", self.index),
                parameters: vec![],
            },
        })
    }

    pub fn definition(&mut self) -> Result<Function> {
        self.eat(Token::Def)?;

        let prototype = self.prototype()?;
        let body = self.expr()?;

        Ok(Function { prototype, body })
    }

    fn prototype(&mut self) -> Result<Prototype> {
        let function_name = self.ident()?;
        self.eat(Token::OpenParen)?;
        let parameters = self.parameters()?;
        self.eat(Token::CloseParen)?;

        Ok(Prototype {
            function_name,
            parameters,
        })
    }

    fn parameters(&mut self) -> Result<Vec<String>> {
        let mut params = vec![];

        while let Token::Identifier(_) = *self.lexer.peek()? {
            let ident = match self.lexer.next_token()? {
                Token::Identifier(ident) => ident,
                _ => unreachable!(),
            };

            params.push(ident);
        }

        Ok(params)
    }

    pub fn extern_(&mut self) -> Result<Prototype> {
        self.eat(Token::Extern)?;

        self.prototype()
    }

    fn expr(&mut self) -> Result<Expr> {
        let left = self.primary()?;

        self.binary_right(0, left)
    }

    fn primary(&mut self) -> Result<Expr> {
        match *self.lexer.peek()? {
            Token::Number(number) => {
                self.lexer.next_token()?;

                Ok(Expr::Number(number))
            }
            Token::OpenParen => {
                self.eat(Token::OpenParen)?;
                let expr = self.expr()?;
                self.eat(Token::CloseParen)?;

                Ok(expr)
            }
            Token::Identifier(_) => self.ident_expr(),
            _ => Err(Unexpected("token, expecting expression")),
        }
    }

    fn binary_right(&mut self, expr_precedence: i32, left: Expr) -> Result<Expr> {
        match self.binary_op()? {
            Some(op) => {
                let token_precedence = self.precedence(op)?;

                if token_precedence < expr_precedence {
                    Ok(left)
                } else {
                    self.lexer.next_token()?; // eat binary op.
                    let right = self.primary()?;
                    let right = match self.binary_op()? {
                        Some(op) => {
                            if token_precedence < self.precedence(op)? {
                                self.binary_right(token_precedence + 1, right)?
                            } else {
                                right
                            }
                        }
                        None => right,
                    };
                    let left = Expr::Binary(op, Box::new(left), Box::new(right));
                    self.binary_right(expr_precedence, left)
                }
            }
            None => Ok(left),
        }
    }

    fn binary_op(&mut self) -> Result<Option<BinaryOp>> {
        let op = match self.lexer.peek()? {
            Token::LessThan => BinaryOp::LessThan,
            Token::Minus => BinaryOp::Minus,
            Token::Plus => BinaryOp::Plus,
            Token::Star => BinaryOp::Times,
            _ => return Ok(None),
        };

        Ok(Some(op))
    }

    fn precedence(&self, op: BinaryOp) -> Result<i32> {
        match self.bin_precedence.get(&op) {
            Some(&precedence) => Ok(precedence),
            None => Err(Undefined("operator")),
        }
    }

    fn ident_expr(&mut self) -> Result<Expr> {
        let name = self.ident()?;

        let ast = match self.lexer.peek()? {
            Token::OpenParen => {
                self.eat(Token::OpenParen)?;
                let args = self.args()?;
                self.eat(Token::CloseParen)?;

                Expr::Call(name, args)
            }
            _ => Expr::Variable(name),
        };

        Ok(ast)
    }

    fn args(&mut self) -> Result<Vec<Expr>> {
        if *self.lexer.peek()? == Token::CloseParen {
            return Ok(vec![]);
        }

        let mut args = vec![self.expr()?];

        while *self.lexer.peek()? == Token::Comma {
            self.eat(Token::Comma)?;
            args.push(self.expr()?);
        }

        Ok(args)
    }

    fn ident(&mut self) -> Result<String> {
        match self.lexer.next_token()? {
            Token::Identifier(ident) => Ok(ident),
            _ => Err(Unexpected("token, expecting identifier")),
        }
    }

    fn eat(&mut self, token: Token) -> Result<()> {
        let current = self.lexer.next_token()?;

        if current != token {
            return Err(Unexpected("token"));
        }

        Ok(())
    }
}
