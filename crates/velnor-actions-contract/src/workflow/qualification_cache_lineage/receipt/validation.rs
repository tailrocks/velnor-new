//! Validation facade for bounded receipt records.

#[path = "validation_layers.rs"]
mod validation_layers;
#[path = "validation_shape.rs"]
mod validation_shape;

pub(super) use validation_layers::validate_lanes;
pub(super) use validation_shape::{
    validate_metadata, validate_node_metadata, validate_receipt_shape,
};
