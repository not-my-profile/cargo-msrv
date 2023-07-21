use camino::Utf8PathBuf;
use std::convert::TryFrom;

use rust_releases::{Release, ReleaseIndex};

use crate::check::Check;
use crate::context::{EnvironmentContext, OutputFormat, VerifyContext};
use crate::error::{CargoMSRVError, TResult};
use crate::manifest::bare_version::BareVersion;
use crate::manifest::reader::{DocumentReader, TomlDocumentReader};
use crate::manifest::CargoManifest;
use crate::outcome::Outcome;
use crate::reporter::event::VerifyResult;
use crate::reporter::Reporter;
use crate::toolchain::ToolchainSpec;

/// Verify whether a Cargo project is compatible with a `rustup run` command,
/// for the (given or specified) `rust_version`.
pub fn verify_msrv(
    reporter: &impl Reporter,
    ctx: &VerifyContext,
    release_index: &ReleaseIndex,
    runner: &impl Check,
) -> TResult<()> {
    let rust_version = ctx.rust_version.clone();

    let bare_version = rust_version.version();
    let version =
        bare_version.try_to_semver(release_index.releases().iter().map(Release::version))?;

    let target = ctx.toolchain.target.as_str();
    let toolchain = ToolchainSpec::new(version, target);

    match runner.check(&toolchain)? {
        Outcome::Success(_) => success(reporter, toolchain),
        Outcome::Failure(_) if ctx.user_output.output_format == OutputFormat::None => {
            failure(reporter, toolchain, rust_version, None)
        }
        Outcome::Failure(f) => failure(reporter, toolchain, rust_version, Some(f.error_message)),
    }
}

// Report the successful verification to the user
fn success(reporter: &impl Reporter, toolchain: ToolchainSpec) -> TResult<()> {
    reporter.report_event(VerifyResult::compatible(toolchain))?;
    Ok(())
}

// Report the failed verification to the user, and return a VerifyFailed error
fn failure(
    reporter: &impl Reporter,
    toolchain: ToolchainSpec,
    rust_version: RustVersion,
    error: Option<String>,
) -> TResult<()> {
    reporter.report_event(VerifyResult::incompatible(toolchain, error))?;

    Err(CargoMSRVError::SubCommandVerify(Error::VerifyFailed(
        VerifyFailed::from(rust_version),
    )))
}

/// Error which can be returned if the verifier deemed the tested Rust version incompatible.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "Crate source was found to be incompatible with Rust version '{}' specified {}", .0.rust_version, .0.source
    )]
    VerifyFailed(VerifyFailed),
}

/// Data structure which contains information about which version failed to verify, and where
/// we obtained this version from.
///
/// It is combination of the Rust version which was tested for compatibility and the source which was
/// used to find this tested Rust version.
#[derive(Debug)]
pub struct VerifyFailed {
    rust_version: BareVersion,
    source: RustVersionSource,
}

impl From<RustVersion> for VerifyFailed {
    fn from(value: RustVersion) -> Self {
        VerifyFailed {
            rust_version: value.rust_version,
            source: value.source,
        }
    }
}

/// A combination of a bare (two- or three component) Rust version and the source which was used to
/// locate this version.
#[derive(Clone, Debug)]
pub struct RustVersion {
    rust_version: BareVersion,
    source: RustVersionSource,
}

impl RustVersion {
    pub fn from_arg(rust_version: BareVersion) -> Self {
        Self {
            rust_version,
            source: RustVersionSource::Arg,
        }
    }

    pub fn try_from_environment(env: &EnvironmentContext) -> TResult<Self> {
        let manifest_path = env.manifest();

        let document = TomlDocumentReader::read_document(&manifest_path)?;
        let manifest = CargoManifest::try_from(document)?;

        manifest
            .minimum_rust_version()
            .ok_or_else(|| CargoMSRVError::NoMSRVKeyInCargoToml(manifest_path.clone()))
            .map(|v| RustVersion {
                rust_version: v.clone(),
                source: RustVersionSource::Manifest(manifest_path.clone()),
            })
    }

    /// Get the bare (two- or three component) version specifying the Rust version.
    pub fn version(&self) -> &BareVersion {
        &self.rust_version
    }

    /// Get the version and discard all else.
    pub fn into_version(self) -> BareVersion {
        self.rust_version
    }
}

/// Source used to obtain a Rust version for the verifier.
#[derive(Clone, Debug, thiserror::Error)]
enum RustVersionSource {
    #[error("as --rust-version argument")]
    Arg,

    #[error("as MSRV in the Cargo manifest located at '{0}'")]
    Manifest(Utf8PathBuf),
}
