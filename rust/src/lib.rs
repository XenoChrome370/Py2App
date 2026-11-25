//! Packaging engine used by the Packager app and the `packager` command.

mod detect;
mod error;
mod fsutil;
mod model;
mod packagers;
mod process;
mod product;

pub use error::{PackagerError, Result};
pub use model::{
    suggested_name, BuildOutcome, BuildRequest, Detection, Language, LanguageInfo, OutputMode,
    Toolchain,
};
pub use process::{active_child_pgid, Log, StdoutLog};

pub fn languages() -> Vec<LanguageInfo> {
    packagers::languages()
}

pub fn toolchains() -> Vec<Toolchain> {
    packagers::toolchains()
}

pub fn resolve(path: &std::path::Path, language: Option<Language>) -> Result<Detection> {
    detect::resolve(path, language)
}

pub fn build(request: &BuildRequest, log: &mut dyn Log) -> Result<BuildOutcome> {
    packagers::build(request, log)
}
