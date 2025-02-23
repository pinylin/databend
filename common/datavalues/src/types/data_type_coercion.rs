// Copyright 2021 Datafuse Labs.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::cmp;

use common_exception::ErrorCode;
use common_exception::Result;

use crate::prelude::DataType;
use crate::DataField;
use crate::DataValueArithmeticOperator;

fn next_size(size: usize) -> usize {
    if size < 8_usize {
        return size * 2;
    }
    size
}

pub fn construct_numeric_type(
    is_signed: bool,
    is_floating: bool,
    byte_size: usize,
) -> Result<DataType> {
    match (is_signed, is_floating, byte_size) {
        (false, false, 1) => Ok(DataType::UInt8),
        (false, false, 2) => Ok(DataType::UInt16),
        (false, false, 4) => Ok(DataType::UInt32),
        (false, false, 8) => Ok(DataType::UInt64),
        (false, true, 4) => Ok(DataType::Float32),
        (false, true, 8) => Ok(DataType::Float64),
        (true, false, 1) => Ok(DataType::Int8),
        (true, false, 2) => Ok(DataType::Int16),
        (true, false, 4) => Ok(DataType::Int32),
        (true, false, 8) => Ok(DataType::Int64),
        (true, true, 1) => Ok(DataType::Float32),
        (true, true, 2) => Ok(DataType::Float32),
        (true, true, 4) => Ok(DataType::Float32),
        (true, true, 8) => Ok(DataType::Float64),

        // TODO support bigint and decimal types, now we just let's overflow
        (false, false, d) if d > 8 => Ok(DataType::Int64),
        (true, false, d) if d > 8 => Ok(DataType::UInt64),
        (_, true, d) if d > 8 => Ok(DataType::Float64),

        _ => Result::Err(ErrorCode::BadDataValueType(format!(
            "Can't construct type from is_signed: {}, is_floating: {}, byte_size: {}",
            is_signed, is_floating, byte_size
        ))),
    }
}

/// Coercion rule for numerical types: The type that both lhs and rhs
/// can be casted to for numerical calculation, while maintaining
/// maximum precision
pub fn numerical_coercion(
    lhs_type: &DataType,
    rhs_type: &DataType,
    allow_overflow: bool,
) -> Result<DataType> {
    let has_float = lhs_type.is_floating() || rhs_type.is_floating();
    let has_integer = lhs_type.is_integer() || rhs_type.is_integer();
    let has_signed = lhs_type.is_signed_numeric() || rhs_type.is_signed_numeric();

    let size_of_lhs = lhs_type.numeric_byte_size()?;
    let size_of_rhs = rhs_type.numeric_byte_size()?;

    let max_size_of_unsigned_integer = cmp::max(
        if lhs_type.is_signed_numeric() {
            0
        } else {
            size_of_lhs
        },
        if rhs_type.is_signed_numeric() {
            0
        } else {
            size_of_rhs
        },
    );

    let max_size_of_signed_integer = cmp::max(
        if !lhs_type.is_signed_numeric() {
            0
        } else {
            size_of_lhs
        },
        if !rhs_type.is_signed_numeric() {
            0
        } else {
            size_of_rhs
        },
    );

    let max_size_of_integer = cmp::max(
        if !lhs_type.is_integer() {
            0
        } else {
            size_of_lhs
        },
        if !rhs_type.is_integer() {
            0
        } else {
            size_of_rhs
        },
    );

    let max_size_of_float = cmp::max(
        if !lhs_type.is_floating() {
            0
        } else {
            size_of_lhs
        },
        if !rhs_type.is_floating() {
            0
        } else {
            size_of_rhs
        },
    );

    let should_double = (has_float && has_integer && max_size_of_integer >= max_size_of_float)
        || (has_signed && max_size_of_unsigned_integer >= max_size_of_signed_integer);

    let mut max_size = if should_double {
        cmp::max(size_of_rhs, size_of_lhs) * 2
    } else {
        cmp::max(size_of_rhs, size_of_lhs)
    };

    if max_size > 8 {
        if allow_overflow {
            max_size = 8
        } else {
            return Result::Err(ErrorCode::BadDataValueType(format!(
                "Can't construct type from {} and {}",
                lhs_type, rhs_type
            )));
        }
    }

    construct_numeric_type(has_signed, has_float, max_size)
}

#[inline]
pub fn numerical_arithmetic_coercion(
    op: &DataValueArithmeticOperator,
    lhs_type: &DataType,
    rhs_type: &DataType,
) -> Result<DataType> {
    // error on any non-numeric type
    if !lhs_type.is_numeric() || !rhs_type.is_numeric() {
        return Result::Err(ErrorCode::BadDataValueType(format!(
            "DataValue Error: Unsupported ({:?}) {} ({:?})",
            lhs_type, op, rhs_type
        )));
    };

    let has_signed = lhs_type.is_signed_numeric() || rhs_type.is_signed_numeric();
    let has_float = lhs_type.is_floating() || rhs_type.is_floating();
    let max_size = cmp::max(lhs_type.numeric_byte_size()?, rhs_type.numeric_byte_size()?);

    match op {
        DataValueArithmeticOperator::Plus | DataValueArithmeticOperator::Mul => {
            construct_numeric_type(has_signed, has_float, next_size(max_size))
        }

        DataValueArithmeticOperator::Modulo => {
            if has_float {
                return Ok(DataType::Float64);
            }
            // From clickhouse: NumberTraits.h
            // If modulo of division can yield negative number, we need larger type to accommodate it.
            // Example: toInt32(-199) % toUInt8(200) will return -199 that does not fit in Int8, only in Int16.
            let result_is_signed = lhs_type.is_signed_numeric();
            let right_size = rhs_type.numeric_byte_size()?;
            let size_of_result = if result_is_signed {
                next_size(right_size)
            } else {
                right_size
            };
            construct_numeric_type(result_is_signed, false, size_of_result)
        }
        DataValueArithmeticOperator::Minus => {
            construct_numeric_type(true, has_float, next_size(max_size))
        }
        DataValueArithmeticOperator::Div => Ok(DataType::Float64),
        DataValueArithmeticOperator::IntDiv => {
            construct_numeric_type(has_signed, has_float, max_size)
        }
    }
}

#[inline]
pub fn datetime_arithmetic_coercion(
    op: &DataValueArithmeticOperator,
    lhs_type: &DataType,
    rhs_type: &DataType,
) -> Result<DataType> {
    let e = Result::Err(ErrorCode::BadDataValueType(format!(
        "DataValue Error: Unsupported date coercion ({:?}) {} ({:?})",
        lhs_type, op, rhs_type
    )));

    if !lhs_type.is_date_or_date_time() && !rhs_type.is_date_or_date_time() {
        return e;
    }

    let mut a = lhs_type.clone();
    let mut b = rhs_type.clone();
    if !a.is_date_or_date_time() {
        a = rhs_type.clone();
        b = lhs_type.clone();
    }

    match op {
        DataValueArithmeticOperator::Plus => Ok(a),

        DataValueArithmeticOperator::Minus => {
            if b.is_numeric() || b.is_interval() {
                Ok(a)
            } else {
                // Date minus Date or DateTime minus DateTime
                Ok(DataType::Int32)
            }
        }
        _ => e,
    }
}

#[inline]
pub fn interval_arithmetic_coercion(
    op: &DataValueArithmeticOperator,
    lhs_type: &DataType,
    rhs_type: &DataType,
) -> Result<DataType> {
    let e = Result::Err(ErrorCode::BadDataValueType(format!(
        "DataValue Error: Unsupported date coercion ({:?}) {} ({:?})",
        lhs_type, op, rhs_type
    )));

    // only allow date/datetime [+/-] interval
    if !(lhs_type.is_date_or_date_time() && rhs_type.is_interval()
        || rhs_type.is_date_or_date_time() && lhs_type.is_interval())
    {
        return e;
    }

    match op {
        DataValueArithmeticOperator::Plus | DataValueArithmeticOperator::Minus => {
            if lhs_type.is_date_or_date_time() {
                Ok(lhs_type.clone())
            } else {
                Ok(rhs_type.clone())
            }
        }
        _ => e,
    }
}

#[inline]
pub fn numerical_unary_arithmetic_coercion(
    op: &DataValueArithmeticOperator,
    val_type: &DataType,
) -> Result<DataType> {
    // error on any non-numeric type
    if !val_type.is_numeric() {
        return Result::Err(ErrorCode::BadDataValueType(format!(
            "DataValue Error: Unsupported ({:?})",
            val_type
        )));
    };

    match op {
        DataValueArithmeticOperator::Plus => Ok(val_type.clone()),
        DataValueArithmeticOperator::Minus => {
            let has_float = val_type.is_floating();
            let has_signed = val_type.is_signed_numeric();
            let numeric_size = val_type.numeric_byte_size()?;
            let max_size = if has_signed {
                numeric_size
            } else {
                next_size(numeric_size)
            };
            construct_numeric_type(true, has_float, max_size)
        }
        other => Result::Err(ErrorCode::UnknownFunction(format!(
            "Unexpected operator:{:?} to unary function",
            other
        ))),
    }
}

// coercion rules for compare operations. This is a superset of all numerical coercion rules.
pub fn compare_coercion(lhs_type: &DataType, rhs_type: &DataType) -> Result<DataType> {
    if lhs_type == rhs_type {
        // same type => equality is possible
        return Ok(lhs_type.clone());
    }

    if lhs_type.is_numeric() && rhs_type.is_numeric() {
        return numerical_coercion(lhs_type, rhs_type, true);
    }

    //  one of is null
    {
        if rhs_type == &DataType::Null {
            return Ok(lhs_type.clone());
        }
        if lhs_type == &DataType::Null {
            return Ok(rhs_type.clone());
        }
    }

    // one of is String and other is number
    if (lhs_type.is_numeric() && rhs_type == &DataType::String)
        || (rhs_type.is_numeric() && lhs_type == &DataType::String)
    {
        return Ok(DataType::Float64);
    }

    // one of is datetime and other is number or string
    {
        if (lhs_type.is_numeric() || lhs_type == &DataType::String)
            && rhs_type.is_date_or_date_time()
        {
            return Ok(rhs_type.clone());
        }

        if (rhs_type.is_numeric() || rhs_type == &DataType::String)
            && lhs_type.is_date_or_date_time()
        {
            return Ok(lhs_type.clone());
        }
    }

    // one of is datetime and other is number or string
    if lhs_type.is_date_or_date_time() || rhs_type.is_date_or_date_time() {
        // one of is datetime
        if matches!(lhs_type, DataType::DateTime32(_))
            || matches!(rhs_type, DataType::DateTime32(_))
        {
            return Ok(DataType::DateTime32(None));
        }

        return Ok(DataType::Date32);
    }

    Err(ErrorCode::IllegalDataType(format!(
        "Can not compare {} with {}",
        lhs_type, rhs_type
    )))
}

// aggregate_types aggregates data types for a multi-argument function.
#[inline]
pub fn aggregate_types(args: &[DataType]) -> Result<DataType> {
    match args.len() {
        0 => Result::Err(ErrorCode::BadArguments("Can't aggregate empty args")),
        1 => Ok(args[0].clone()),
        _ => {
            let left = args[0].clone();
            let right = aggregate_types(&args[1..args.len()])?;
            merge_types(&left, &right)
        }
    }
}

pub fn merge_types(lhs_type: &DataType, rhs_type: &DataType) -> Result<DataType> {
    match (lhs_type, rhs_type) {
        (DataType::Null, _) => Ok(rhs_type.clone()),
        (_, DataType::Null) => Ok(lhs_type.clone()),
        (DataType::List(a), DataType::List(b)) => {
            if a.name() != b.name() {
                return Result::Err(ErrorCode::BadDataValueType(format!(
                    "Can't merge types from {} and {}",
                    lhs_type, rhs_type
                )));
            }
            let typ = merge_types(a.data_type(), b.data_type())?;
            Ok(DataType::List(Box::new(DataField::new(
                a.name(),
                typ,
                a.is_nullable() || b.is_nullable(),
            ))))
        }
        (DataType::Struct(a), DataType::Struct(b)) => {
            if a.len() != b.len() {
                return Result::Err(ErrorCode::BadDataValueType(format!(
                    "Can't merge types from {} and {}, because they have different sizes",
                    lhs_type, rhs_type
                )));
            }
            let fields = a
                .iter()
                .zip(b.iter())
                .map(|(a, b)| {
                    if a.name() != b.name() {
                        return Result::Err(ErrorCode::BadDataValueType(format!(
                            "Can't merge types from {} and {}",
                            lhs_type, rhs_type
                        )));
                    }
                    let typ = merge_types(a.data_type(), b.data_type())?;
                    Ok(DataField::new(
                        a.name(),
                        typ,
                        a.is_nullable() || b.is_nullable(),
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(DataType::Struct(fields))
        }
        _ => {
            if lhs_type == rhs_type {
                return Ok(lhs_type.clone());
            }
            if lhs_type.is_numeric() && rhs_type.is_numeric() {
                numerical_coercion(lhs_type, rhs_type, false)
            } else {
                Result::Err(ErrorCode::BadDataValueType(format!(
                    "Can't merge types from {} and {}",
                    lhs_type, rhs_type
                )))
            }
        }
    }
}
