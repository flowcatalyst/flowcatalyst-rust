//! Guest bindings for the `flowcatalyst:function` WIT package (world
//! `imports`), from the published package at the repository root.

wit_bindgen::generate!({
    path: "../../../../../wit/flowcatalyst-function",
    world: "imports",
});

pub use flowcatalyst::function::*;
