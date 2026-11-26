// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/parser.rs

use super::env::{Function, ParseEnvironment};
use super::error::{ParseError, ParseErrorKind};
use super::lexer::LexicalAnalyzer;
use super::node::{BinaryOperator, Node};
use super::token::{Associativity, Token, TokenKind};
use super::value::Value;

pub(crate) const MAX_DEPTH: usize = 50;
pub(crate) const MAX_LENGTH: usize = 21000;

struct ParseContext<'a> {
    expression: &'a str,
    lexer: LexicalAnalyzer,
    operands: Vec<Node>,
    operators: Vec<Token>,
    last_token: Option<Token>,
    env: &'a dyn ParseEnvironment,
}

pub(crate) fn parse(
    expression: &str,
    env: &dyn ParseEnvironment,
) -> Result<Option<Node>, ParseError> {
    if expression.encode_utf16().count() > MAX_LENGTH {
        return Err(ParseError::without_token(ParseErrorKind::ExceededMaxLength));
    }

    let mut context = ParseContext {
        expression,
        lexer: LexicalAnalyzer::new(expression),
        operands: Vec::new(),
        operators: Vec::new(),
        last_token: None,
        env,
    };

    while let Some(token) = context.lexer.next_token() {
        if token.kind == TokenKind::Unexpected {
            return Err(ParseError::with_token(
                ParseErrorKind::UnexpectedSymbol,
                &token,
                expression,
            ));
        } else if token.is_operator() {
            push_operator(&mut context, token.clone())?;
        } else {
            push_operand(&mut context, &token)?;
        }
        context.last_token = Some(token);
    }

    let Some(last_token) = context.last_token.clone() else {
        return Ok(None);
    };

    if !context.operators.is_empty() {
        let unexpected_last_token = match last_token.kind {
            TokenKind::EndGroup | TokenKind::EndIndex | TokenKind::EndParameters => false,
            TokenKind::Function => true,
            _ => last_token.is_operator(),
        };
        if unexpected_last_token || context.lexer.has_unclosed_tokens() {
            return Err(ParseError::with_token(
                ParseErrorKind::UnexpectedEndOfExpression,
                &last_token,
                expression,
            ));
        }
    }

    while !context.operators.is_empty() {
        flush_top_operator(&mut context)?;
    }

    let Ok([result]) = <[Node; 1]>::try_from(std::mem::take(&mut context.operands)) else {
        return Err(ParseError::with_token(
            ParseErrorKind::UnexpectedEndOfExpression,
            &last_token,
            expression,
        ));
    };
    Ok(Some(result))
}

fn push_operand(context: &mut ParseContext<'_>, token: &Token) -> Result<(), ParseError> {
    let node = match token.kind {
        TokenKind::Function => {
            let Some(function) = context.env.function(&token.raw_value) else {
                return Err(ParseError::with_token(
                    ParseErrorKind::UnsupportedFunction,
                    token,
                    context.expression,
                ));
            };
            Node::Function {
                function,
                name: token.raw_value.clone(),
                args: Vec::new(),
            }
        }
        TokenKind::NamedValue => {
            let Some(root) = context.env.root_context(&token.raw_value) else {
                return Err(ParseError::with_token(
                    ParseErrorKind::UnsupportedContext,
                    token,
                    context.expression,
                ));
            };
            Node::NamedValue(root)
        }
        TokenKind::Wildcard => Node::Wildcard,
        TokenKind::PropertyName => Node::Literal(Value::String(token.raw_value.clone())),
        TokenKind::Null | TokenKind::Boolean | TokenKind::Number | TokenKind::String => {
            Node::Literal(token.parsed_value.clone().unwrap_or(Value::Null))
        }
        other => {
            return Err(ParseError::internal(
                context.expression,
                &format!("token kind {other:?} is not an operand"),
            ));
        }
    };
    context.operands.push(node);
    Ok(())
}

fn push_operator(context: &mut ParseContext<'_>, token: Token) -> Result<(), ParseError> {
    if token.associativity() == Associativity::LeftToRight {
        let precedence = token.precedence();
        while let Some(top) = context.operators.last() {
            if precedence <= top.precedence()
                && !matches!(
                    top.kind,
                    TokenKind::StartGroup
                        | TokenKind::StartIndex
                        | TokenKind::StartParameters
                        | TokenKind::Separator
                )
            {
                flush_top_operator(context)?;
                continue;
            }
            break;
        }
    }

    let kind = token.kind;
    context.operators.push(token);

    if matches!(
        kind,
        TokenKind::EndGroup | TokenKind::EndIndex | TokenKind::EndParameters
    ) {
        flush_top_operator(context)?;
    }
    Ok(())
}

fn flush_top_operator(context: &mut ParseContext<'_>) -> Result<(), ParseError> {
    match context.operators.last().map(|token| token.kind) {
        Some(TokenKind::EndIndex) => return flush_top_end_index(context),
        Some(TokenKind::EndGroup) => {
            pop_operator(context, TokenKind::EndGroup)?;
            pop_operator(context, TokenKind::StartGroup)?;
            return Ok(());
        }
        Some(TokenKind::EndParameters) => return flush_top_end_parameters(context),
        _ => {}
    }

    let operator = context
        .operators
        .pop()
        .ok_or_else(|| ParseError::internal(context.expression, "operator stack underflow"))?;
    let operands = pop_operands(context, operator.operand_count())?;

    let node = match operator.kind {
        TokenKind::LogicalOperator => logical_node(context.expression, &operator, operands)?,
        TokenKind::Dereference | TokenKind::StartIndex => {
            let (left, right) = operand_pair(context.expression, operands)?;
            Node::Index(Box::new(left), Box::new(right))
        }
        other => {
            return Err(ParseError::internal(
                context.expression,
                &format!("unexpected operator kind {other:?}"),
            ));
        }
    };
    push_checked(context, node)
}

fn push_checked(context: &mut ParseContext<'_>, node: Node) -> Result<(), ParseError> {
    check_max_depth(&node)?;
    context.operands.push(node);
    Ok(())
}

fn logical_node(
    expression: &str,
    operator: &Token,
    operands: Vec<Node>,
) -> Result<Node, ParseError> {
    let binary = match operator.raw_value.as_str() {
        "!" => {
            let [operand] = <[Node; 1]>::try_from(operands).map_err(|_| {
                ParseError::internal(expression, "unary operator needs one operand")
            })?;
            return Ok(Node::Not(Box::new(operand)));
        }
        "&&" | "||" => {
            let is_and = operator.raw_value == "&&";
            let mut parameters = Vec::new();
            for operand in operands {
                match (operand, is_and) {
                    (Node::And(nested), true) | (Node::Or(nested), false) => {
                        parameters.extend(nested);
                    }
                    (operand, _) => parameters.push(operand),
                }
            }
            return Ok(if is_and {
                Node::And(parameters)
            } else {
                Node::Or(parameters)
            });
        }
        "==" => BinaryOperator::Equal,
        "!=" => BinaryOperator::NotEqual,
        ">" => BinaryOperator::GreaterThan,
        ">=" => BinaryOperator::GreaterThanOrEqual,
        "<" => BinaryOperator::LessThan,
        "<=" => BinaryOperator::LessThanOrEqual,
        other => {
            return Err(ParseError::internal(
                expression,
                &format!("unexpected logical operator '{other}'"),
            ));
        }
    };
    let (left, right) = operand_pair(expression, operands)?;
    Ok(Node::Binary(binary, Box::new(left), Box::new(right)))
}

fn operand_pair(expression: &str, operands: Vec<Node>) -> Result<(Node, Node), ParseError> {
    let [left, right] = <[Node; 2]>::try_from(operands)
        .map_err(|_| ParseError::internal(expression, "binary operator needs two operands"))?;
    Ok((left, right))
}

fn flush_top_end_index(context: &mut ParseContext<'_>) -> Result<(), ParseError> {
    pop_operator(context, TokenKind::EndIndex)?;
    let operator = pop_operator(context, TokenKind::StartIndex)?;
    let operands = pop_operands(context, operator.operand_count())?;
    let (left, right) = operand_pair(context.expression, operands)?;
    push_checked(context, Node::Index(Box::new(left), Box::new(right)))
}

fn flush_top_end_parameters(context: &mut ParseContext<'_>) -> Result<(), ParseError> {
    pop_operator(context, TokenKind::EndParameters)?;

    let parameters = if context.last_token.as_ref().map(|token| token.kind)
        == Some(TokenKind::StartParameters)
    {
        Vec::new()
    } else {
        let mut parameter_count = 1;
        while context.operators.last().map(|token| token.kind) == Some(TokenKind::Separator) {
            parameter_count += 1;
            context.operators.pop();
        }
        pop_operands(context, parameter_count)?
    };

    let Some(Node::Function {
        function,
        name,
        args,
    }) = context.operands.last_mut()
    else {
        return Err(ParseError::internal(
            context.expression,
            "expected a function node on the operand stack",
        ));
    };
    args.extend(parameters);
    let (min, max) = function.arity();
    let arg_count = args.len();
    let non_literal_from_json = *function == Function::FromJson
        && !matches!(args.as_slice(), [Node::Literal(Value::String(_))]);
    let name = name.clone();

    let start = pop_operator(context, TokenKind::StartParameters)?;
    if arg_count < min {
        return Err(ParseError::with_token(
            ParseErrorKind::TooFewParameters,
            &start,
            context.expression,
        ));
    }
    if arg_count > max {
        return Err(ParseError::with_token(
            ParseErrorKind::TooManyParameters,
            &start,
            context.expression,
        ));
    }
    if non_literal_from_json {
        return Err(ParseError::unsupported_function(&name));
    }
    context.operands.last().map_or(Ok(()), check_max_depth)
}

fn pop_operands(context: &mut ParseContext<'_>, count: usize) -> Result<Vec<Node>, ParseError> {
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        let operand = context
            .operands
            .pop()
            .ok_or_else(|| ParseError::internal(context.expression, "operand stack underflow"))?;
        result.push(operand);
    }
    result.reverse();
    Ok(result)
}

fn pop_operator(context: &mut ParseContext<'_>, expected: TokenKind) -> Result<Token, ParseError> {
    let token = context
        .operators
        .pop()
        .ok_or_else(|| ParseError::internal(context.expression, "operator stack underflow"))?;
    if token.kind != expected {
        return Err(ParseError::internal(
            context.expression,
            &format!(
                "expected operator {expected:?} to be popped, found {:?}",
                token.kind
            ),
        ));
    }
    Ok(token)
}

fn check_max_depth(node: &Node) -> Result<(), ParseError> {
    if node.depth() > MAX_DEPTH {
        return Err(ParseError::without_token(ParseErrorKind::ExceededMaxDepth));
    }
    Ok(())
}
