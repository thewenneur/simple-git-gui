//! Everything that talks to git. The UI never spawns processes itself.

pub mod config;
pub mod diff;
pub mod ignore;
pub mod ops;
pub mod repo;
pub mod runner;
pub mod status;
pub mod undo;
