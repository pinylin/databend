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

use common_datavalues::prelude::*;
use common_exception::Result;
use common_functions::scalars::SubstringFunction;

use super::run_tests;
use super::Test;

#[test]
fn test_substring_function() -> Result<()> {
    let schema = DataSchemaRefExt::create(vec![
        DataField::new("a", DataType::String, false),
        DataField::new("b", DataType::Int64, false),
        DataField::new("c", DataType::UInt64, false),
    ]);

    let tests = vec![
        Test {
            name: "substring-abcde-passed",
            display: "substring",
            nullable: true,
            arg_names: vec!["a", "b", "c"],
            columns: vec![
                Series::new(vec!["abcde"]).into(),
                Series::new(vec![2_i64]).into(),
                Series::new(vec![3_u64]).into(),
            ],
            func: SubstringFunction::try_create("substring")?,
            expect: Series::new(vec!["bcd"]).into(),
            error: "",
        },
        Test {
            name: "substring-abcde-passed",
            display: "substring",
            nullable: true,
            arg_names: vec!["a", "b", "c"],
            columns: vec![
                Series::new(vec!["abcde"]).into(),
                Series::new(vec![1_i64]).into(),
                Series::new(vec![3_u64]).into(),
            ],
            func: SubstringFunction::try_create("substring")?,
            expect: Series::new(vec!["abc"]).into(),
            error: "",
        },
        Test {
            name: "substring-abcde-passed",
            display: "substring",
            nullable: true,
            arg_names: vec!["a", "b"],
            columns: vec![
                Series::new(vec!["abcde"]).into(),
                Series::new(vec![2_i64]).into(),
            ],

            func: SubstringFunction::try_create("substring")?,
            expect: Series::new(vec!["bcde"]).into(),
            error: "",
        },
        Test {
            name: "substring-1234567890-passed",
            display: "substring",
            nullable: true,
            arg_names: vec!["a", "b", "c"],
            columns: vec![
                Series::new(vec!["1234567890"]).into(),
                Series::new(vec![-3_i64]).into(),
                Series::new(vec![3_u64]).into(),
            ],

            func: SubstringFunction::try_create("substring")?,
            expect: Series::new(vec!["890"]).into(),
            error: "",
        },
    ];
    run_tests(tests, schema)
}
