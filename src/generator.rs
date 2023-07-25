use std::{
    fmt,
    path::{Path, PathBuf},
};

use eyre::Context;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{
    manifest::{Dependency, Manifest},
    package::PackageStore,
};

pub const BUILD_DIRECTORY: &str = "proto/build";

/// The language used for code generation
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    Rust,
}

impl Language {
    pub fn build_directory(&self) -> PathBuf {
        Path::new(BUILD_DIRECTORY).join(self.to_string())
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", serde_typename::to_str(&self).unwrap_or("unknown"))
    }
}

/// Backend used to generate code bindings
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Generator {
    Tonic,
}

impl Generator {
    pub const TONIC_INCLUDE_FILE: &str = "mod.rs";

    pub async fn run(&self, dependency: &Dependency, out: &Path) -> eyre::Result<()> {
        let protoc = protobuf_src::protoc();
        std::env::set_var("PROTOC", protoc.clone());

        match self {
            Generator::Tonic => {
                let out = out.join(dependency.package.as_str());

                fs::remove_dir_all(&out).await.ok();

                fs::create_dir_all(&out)
                    .await
                    .wrap_err("failed to recreate dependency output directory")?;

                let package = PackageStore::locate(&dependency.package);
                let protos = PackageStore::collect(&package).await;

                let includes = &[package];

                tonic_build::configure()
                    .build_client(true)
                    .build_server(true)
                    .build_transport(true)
                    .compile_well_known_types(true)
                    .out_dir(&out)
                    .include_file(Self::TONIC_INCLUDE_FILE)
                    .compile(&protos, includes)?;
            }
        }

        Ok(())
    }
}

/// Generate the code bindings for a language
pub async fn generate(language: Language) -> eyre::Result<()> {
    let manifest = Manifest::read().await?;

    tracing::info!(":: initializing code generator for {language}");

    // Only tonic is supported right now
    let generator = Generator::Tonic;

    let out = {
        let out = language.build_directory();

        fs::remove_dir_all(&out).await.ok();

        fs::create_dir_all(&out).await.wrap_err(eyre::eyre!(
            "failed to create clean build directory {} for {language}",
            out.canonicalize()?.to_string_lossy()
        ))?;

        out
    };

    for dependency in manifest.dependencies {
        generator
            .run(&dependency, &out)
            .await
            .wrap_err_with(|| format!("failed to generate bindings for {}", dependency.package))?;

        tracing::info!(
            ":: compiled {}",
            PackageStore::locate(&dependency.package).display()
        );
    }

    Ok(())
}

/// Blocking buffrs generation implementation
///
/// Important: Only use this inside of cargo build scripts!
pub fn generate_blocking(language: Language) -> eyre::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(generate(language))
}

/// Include the rust language bindings of a buffrs dependency
///
/// You must specify the buffrs dependency package id.
///
/// ```rust,ignore
/// mod protos {
///     buffrs::include!("demo");
/// }
/// ```
#[macro_export]
macro_rules! include {
    ($package:expr) => {
        ::std::include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/proto/build/rust/",
            $package,
            "/mod.rs"
        ));
    };
}
