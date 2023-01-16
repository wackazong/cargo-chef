use std::collections::HashSet;

use super::ParsedManifest;

/// All local dependencies are emptied out when running `prepare`.
/// We do not want the recipe file to change if the only difference with
/// the previous docker build attempt is the version of a local crate
/// encoded in `Cargo.lock` (while the remote dependency tree
/// is unchanged) or in the corresponding `Cargo.toml` manifest.
/// We replace versions of local crates in `Cargo.lock` and in all `Cargo.toml`s, including
/// when specified as dependency of another crate in the workspace.
pub(super) fn mask_local_crate_versions(
    member: &Option<String>,
    manifests: &mut [ParsedManifest],
    lock_file: &mut Option<toml::Value>,
) {
    let local_package_names = parse_local_crate_names(member, manifests);
    mask_local_versions_in_manifests(manifests, &local_package_names);
    if let Some(l) = lock_file {
        mask_local_versions_in_lockfile(l, &local_package_names);
    }
}

/// Dummy version used for all local crates.
const CONST_VERSION: &str = "0.0.1";

fn mask_local_versions_in_lockfile(
    lock_file: &mut toml::Value,
    local_package_names: &HashSet<String>,
) {
    if let Some(packages) = lock_file
        .get_mut("package")
        .and_then(|packages| packages.as_array_mut())
    {
        packages
            .iter_mut()
            // Find all local crates
            .filter(|package| {
                package
                    .get("name")
                    .map(|name| {
                        if let toml::Value::String(name) = name {
                            local_package_names.contains(name)
                        } else {
                            false
                        }
                    })
                    .unwrap_or_default()
            })
            // Mask the version
            .for_each(|package| {
                if let Some(version) = package.get_mut("version") {
                    *version = toml::Value::String(CONST_VERSION.to_string())
                }
            });
    }
}

fn mask_local_versions_in_manifests(
    manifests: &mut [ParsedManifest],
    local_package_names: &HashSet<String>,
) {
    for manifest in manifests.iter_mut() {
        if let Some(package) = manifest.contents.get_mut("package") {
            if let Some(version) = package.get_mut("version") {
                if version.as_str().is_some() {
                    *version = toml::Value::String(CONST_VERSION.to_string());
                }
            }
        }
        mask_local_dependency_versions(local_package_names, manifest);
    }
}

fn mask_local_dependency_versions(
    local_package_names: &HashSet<String>,
    manifest: &mut ParsedManifest,
) {
    fn _mask(local_package_names: &HashSet<String>, toml_value: &mut toml::Value) {
        for dependency_key in ["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(dependencies) = toml_value.get_mut(dependency_key) {
                for local_package in local_package_names.iter() {
                    if let Some(local_dependency) = dependencies.get_mut(local_package) {
                        if let Some(version) = local_dependency.get_mut("version") {
                            *version = toml::Value::String(CONST_VERSION.to_string());
                        }
                    }
                }
            }
        }
    }

    // There are three ways to specify dependencies:
    // - top-level
    // ```toml
    // [dependencies]
    // # [...]
    // ```
    // - target-specific (e.g. Windows-only)
    // ```toml
    // [target.'cfg(windows)'.dependencies]
    // winhttp = "0.4.0"
    // ```
    // The inner structure for target-specific dependencies mirrors the structure expected
    // for top-level dependencies.
    // Check out cargo's documentation (https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html)
    // for more details.
    _mask(local_package_names, &mut manifest.contents);
    if let Some(targets) = manifest.contents.get_mut("target") {
        if let Some(target_table) = targets.as_table_mut() {
            for (_, target_config) in target_table.iter_mut() {
                _mask(local_package_names, target_config)
            }
        }
    }

    // The third way to specify dependencies was introduced in rust 1.64: workspace inheritance.
    // ```toml
    // [workspace.dependencies]
    // anyhow = "1.0.66"
    // project_a = { path = "project_a", version = "0.2.0" }
    // ```
    // Check out cargo's documentation (https://doc.rust-lang.org/cargo/reference/workspaces.html#the-workspacedependencies-table)
    // for more details.
    if let Some(workspace) = manifest.contents.get_mut("workspace") {
        // Mask the workspace package version
        if let Some(package) = workspace.get_mut("package") {
            if let Some(version) = package.get_mut("version") {
                *version = toml::Value::String(CONST_VERSION.to_string());
            }
        }
        // Mask the local crates in the workspace dependencies
        _mask(local_package_names, workspace);
    }
}

fn parse_local_crate_names(
    member: &Option<String>,
    manifests: &[ParsedManifest],
) -> HashSet<String> {
    let mut local_package_names = HashSet::new();
    for manifest in manifests.iter() {
        if let Some(package) = manifest.contents.get("package") {
            if let Some(name) = package.get("name") {
                if let toml::Value::String(name) = name {
                    if let Some(member) = member {
                        if member != name {
                            // just evaluate the selected package for local dependencies if user specifed --bin option
                            continue;
                        }
                        // evaluate the dependencies sections and extract local path dependencies
                        for dependency_key in
                            ["dependencies", "dev-dependencies", "build-dependencies"]
                        {
                            if let Some(dependencies) = manifest.contents.get(dependency_key) {
                                if let toml::Value::Table(dependencies) = dependencies {
                                    for (key, value) in dependencies.iter() {
                                        // local dependencies have a path
                                        if let Some(_) = value.get("path") {
                                            local_package_names.insert(key.to_owned());
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        local_package_names.insert(name.to_owned());
                    }
                }
            }
        }
    }
    local_package_names
}
