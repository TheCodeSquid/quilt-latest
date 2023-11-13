use std::env;

use anyhow::{Context, Result};
use semver::Version;
use serde::Deserialize;
use serde_json::Value;

const META_URL: &str = "https://meta.quiltmc.org/v3/versions";
const MAVEN_URL: &str = "https://maven.quiltmc.org/repository/release";
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

type Map<T> = serde_json::Map<String, T>;

#[derive(Deserialize, Debug)]
struct MetaEntry {
    version: String,
    #[serde(flatten)]
    extra: Map<Value>,
}

#[derive(Deserialize, Debug)]
struct MavenPackage {
    versioning: MavenVersioning,
}

#[derive(Deserialize, Debug)]
struct MavenVersioning {
    versions: MavenVersions,
}

#[derive(Deserialize, Debug)]
struct MavenVersions {
    version: Vec<Version>,
}

#[derive(Debug)]
struct Versions {
    minecraft: String,
    loom: String,
    loader: String,
    mappings: String,
    qfapi: Option<String>,
}

struct Client {
    agent: ureq::Agent,
}

impl Client {
    fn new() -> Client {
        let agent = ureq::AgentBuilder::new().user_agent(USER_AGENT).build();

        Client { agent }
    }

    fn meta<S: AsRef<str>>(&self, path: S) -> Result<Vec<MetaEntry>> {
        let url = format!("{}/{}", META_URL, path.as_ref());
        let versions: Vec<MetaEntry> = self.agent.get(&url).call()?.into_json()?;
        Ok(versions)
    }

    fn maven<S: AsRef<str>>(&self, pkg: S) -> Result<Vec<Version>> {
        let url = format!(
            "{}/{}/maven-metadata.xml",
            MAVEN_URL,
            pkg.as_ref().replace('.', "/")
        );

        let text = self.agent.get(&url).call()?.into_string()?;
        let pkg: MavenPackage = quick_xml::de::from_str(&text)?;
        let mut versions = pkg.versioning.versions.version;
        versions.sort();
        Ok(versions.into_iter().rev().collect())
    }
}

fn main() -> Result<()> {
    let client = Client::new();

    // Versions from quilt meta

    let minecraft = if let Some(version) = env::args().nth(1) {
        version
    } else {
        let version = client
            .meta("/game")?
            .into_iter()
            .find(|entry| entry.extra.get("stable").and_then(|v| v.as_bool()) == Some(true))
            .map(|v| v.version)
            .with_context(|| "no stable Minecraft versions (???)")?;
        eprintln!("Using latest Minecraft version ({version})");
        version
    };

    let loader = client
        .meta("/loader")?
        .into_iter()
        .map(|v| v.version)
        .find(|v| !v.contains('-'))
        .with_context(|| "no loaders (???)")?;

    let mappings = client
        .meta(format!("/quilt-mappings/{minecraft}"))?
        .into_iter()
        .next()
        .map(|v| v.version)
        .with_context(|| format!("no mappings compatible with Minecraft version {minecraft}"))?;

    // Versions from quilt maven

    let loom = client
        .maven("org.quiltmc.loom")?
        .into_iter()
        .next()
        .map(|v| v.to_string())
        .with_context(|| "no loom versions (???)")?;

    let qfapi = client
        .maven("org.quiltmc.quilted-fabric-api.quilted-fabric-api")?
        .into_iter()
        .find(|v| v.build.contains(&minecraft))
        .map(|v| v.to_string());

    let catalog = format_gradle_catalog(&Versions {
        minecraft,
        loader,
        mappings,
        loom,
        qfapi,
    });

    println!("{catalog}");

    Ok(())
}

#[rustfmt::skip]
fn format_gradle_catalog(
    Versions {
        minecraft,
        loader,
        mappings,
        loom,
        qfapi
    }: &Versions,
) -> String {
    let (qfapi_version, qfapi_lib_comment) = if let Some(qfapi) = qfapi {
        (
            format!(r#"quilted_fabric_api = "{qfapi}""#),
            "".to_string()
        )
    } else {
        (
            "# Compatible Quilted Fabric API not found; check manually.".to_string(),
            "# ".to_string()
        )
    };
    
    format!(
r#"[versions]
minecraft = "{minecraft}"
quilt_loader = "{loader}"
quilt_mappings = "{mappings}"

{qfapi_version}

[libraries]
minecraft = {{ module = "com.mojang:minecraft", version.ref = "minecraft" }}
quilt_loader = {{ module = "org.quiltmc:quilt-loader", version.ref = "quilt_loader" }}
quilt_mappings = {{ module = "org.quiltmc:quilt-mappings", version.ref = "quilt_mappings" }}
        
{qfapi_lib_comment}quilted_fabric_api = {{ module = "org.quiltmc.quilted-fabric-api:quilted-fabric-api", version.ref = "quilted_fabric_api" }}

[plugins]
quilt_loom = {{ id = "org.quiltmc.loom", version = "{loom}" }}"#
    )
}
