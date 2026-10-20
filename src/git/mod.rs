//! Everything that talks to git. The UI never spawns processes itself.

pub mod branches;
pub mod config;
pub mod conflict;
pub mod diff;
pub mod graph;
pub mod ignore;
pub mod integrate;
pub mod lfs;
pub mod ops;
pub mod remotes;
pub mod repo;
pub mod rewrite;
pub mod runner;
pub mod status;
pub mod submodules;
pub mod templates;
pub mod undo;
