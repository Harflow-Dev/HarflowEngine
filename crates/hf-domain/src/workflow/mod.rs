//! Pure values and rules of workflow definitions: an ordered JSON model, vocabulary,
//! refusals, name and Git rules, graph algorithms, dependency trees and stored
//! qualification. No parsing, rendering or effects live here.
pub mod json;
pub mod json_int;
pub mod refusal;
pub mod rule_graph;
pub mod rule_shape;
pub mod text;
pub mod tree;
pub mod vocab;
