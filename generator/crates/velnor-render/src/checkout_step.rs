// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::rules::{RenderedStep, Value};
use crate::steps::CHECKOUT;

pub(crate) fn checkout_step(depth: u32, sparse: Option<&[String]>) -> RenderedStep {
    let mut with = vec![
        ("fetch-depth".to_owned(), Value::Int(u64::from(depth))),
        ("persist-credentials".to_owned(), Value::Bool(false)),
    ];
    if let Some(paths) = sparse {
        with.push((
            "sparse-checkout".to_owned(),
            Value::Text(format!("{}\n", paths.join("\n"))),
        ));
        with.push(("sparse-checkout-cone-mode".to_owned(), Value::Bool(false)));
    }
    RenderedStep {
        name: "Check out".to_owned(),
        uses: Some(CHECKOUT.to_owned()),
        with,
        ..RenderedStep::default()
    }
}
