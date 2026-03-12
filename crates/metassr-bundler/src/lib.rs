use anyhow::{anyhow, Result};
use serde_json::json;
use std::{
    collections::HashMap,
    ffi::OsStr,
    marker::Sized,
    path::Path, 
    sync::Arc, vec,
};

use rspack::builder::{Builder as _, Devtool, OutputOptionsBuilder};
use rspack_core::{Compiler, Experiments, Filename, PublicPath, LibraryOptions,
    LibraryType, Mode, ModuleOptions, ModuleRule, ModuleRuleEffect, ModuleRuleUse,
    ModuleRuleUseLoader, Resolve, RuleSetCondition, ModuleType};
use rspack_paths::Utf8Path;
use rspack_regex::RspackRegex;
use rspack_fs::{ WritableFileSystem, NativeFileSystem };

#[derive(Debug)]
pub struct WebBundler<'a> {
    pub targets: HashMap<String, &'a Path>,
    pub dist_path: &'a Path,
}

impl<'a> WebBundler<'a> {
    pub fn new<S>(targets: &'a HashMap<String, String>, dist_path: &'a S) -> Result<Self>
    where
        S: AsRef<OsStr> + ?Sized,
    {
        let mut non_found_files = vec![];
        let targets: HashMap<String, &Path> = targets
            .iter()
            .map(|(k, path)| {
                let path = Path::new(path);
                if !path.exists() {
                    non_found_files.push(path.to_str().unwrap());
                }
                (k.into(), path)
            })
            .collect();

        if !non_found_files.is_empty() {
            return Err(anyhow!(
                "[bundler] Non Exist files found: {:?}",
                non_found_files
            ));
        }

        Ok(Self {
            targets,
            dist_path: Path::new(dist_path),
        })
    }

    pub fn exec(&self) -> Result<()> {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => match handle.runtime_flavor() {
                tokio::runtime::RuntimeFlavor::MultiThread => {
                    tokio::task::block_in_place(|| {
                        handle.block_on(async { self.exec_async().await })
                    })
                }
                _ => {
                    std::thread::scope(|s| {
                        s.spawn(|| {
                            tokio::runtime::Runtime::new()
                                .map_err(|e| anyhow!("Failed to create runtime: {:?}", e))?
                                .block_on(async { self.exec_async().await })
                        })
                        .join()
                        .map_err(|_| anyhow!("Bundler thread panicked"))?
                    })
                }
            },
            Err(_) => {
                tokio::runtime::Runtime::new()
                    .map_err(|e| anyhow!("Failed to create runtime: {:?}", e))?
                    .block_on(async { self.exec_async().await })
            }
        }
    }
    async fn exec_async(&self) -> Result<()> {
        let mut builder = Compiler::builder();
        
        let context = Utf8Path::new(".");
        
        for (name, path) in &self.targets {
            let path_str = path.to_str().ok_or_else(|| anyhow!("Invalid path"))?;
            builder.entry(name.as_str(), path_str);
        }
        
        let resolve_options = Resolve {
            modules: Some(vec!["node_modules".to_string()]),
            extensions: Some(vec![
                ".js".into(), ".jsx".into(), ".tsx".into(), ".ts".into()
            ]),
            ..Default::default()
        };

        let js_regex = RspackRegex::new(r#"\.(jsx|js)$"#)
            .map_err(|e| anyhow!("Invalid JS rule regex: {}", e))?;
        let js_rule = ModuleRule {
            test: Some(RuleSetCondition::Regexp(js_regex)),
            exclude: Some(RuleSetCondition::Regexp(RspackRegex::new(r#"node_modules"#).map_err(|e| anyhow!("Skip node modules: {}", e))?)),
            effect: ModuleRuleEffect {
                r#use: ModuleRuleUse::Array(vec![ModuleRuleUseLoader {
                loader: "builtin:swc-loader".to_string(),
                options: Some(json!({
                    "jsc": {
                        "parser": {
                            "syntax": "ecmascript",
                            "jsx": true,
                            "dynamicImport": true, 
                        },
                        "transform": {
                            "react": {
                                "runtime": "automatic",
                                "throwIfNamespace": true,
                            }
                        }
                    }
                }).to_string()),
                }]),
                r#type: Some(ModuleType::from("javascript/auto")),
                ..Default::default()
            },
            ..Default::default()
        };

        let ts_regex = RspackRegex::new(r#"\.(tsx|ts)$"#)
            .map_err(|e| anyhow!("Invalid TS rule regex: {}", e))?;
        let ts_rule = ModuleRule {
            test: Some(RuleSetCondition::Regexp(ts_regex)),
            exclude: Some(RuleSetCondition::Regexp(RspackRegex::new(r#"node_modules"#).map_err(|e| anyhow!("Skip node modules: {}", e))?)),
            effect: ModuleRuleEffect {
                r#use: ModuleRuleUse::Array(vec![ModuleRuleUseLoader {
                    loader: "builtin:swc-loader".to_string(),
                    options: Some(json!({
                        "jsc": {
                            "parser": {
                                "syntax": "typescript",
                                "tsx": true,
                                "decorators": true
                            },
                            "transform": {
                                "react": {
                                    "runtime": "automatic",
                                    "throwIfNamespace": true,
                                }
                            }
                        }
                    }).to_string()),
                }]),
                r#type: Some(ModuleType::from("javascript/auto")),
                ..Default::default()
            },
            ..Default::default()
        };

        
        let fs = Arc::new(NativeFileSystem::new(false));
        let dist_utf8 = Utf8Path::new(self.dist_path.to_str().ok_or_else(|| anyhow!("Invalid dist path"))?);

        fs.create_dir_all(dist_utf8).await
            .map_err(|e| anyhow!("Failed to create output directory: {:?}", e))?;


        builder
            .module(ModuleOptions::builder()
            .rules(vec![js_rule, ts_rule]))
            .context(context)
            .experiments(Experiments::builder().css(true))
            .mode(Mode::Production)
            .devtool(Devtool::SourceMap)
            .enable_loader_swc()
            .output_filesystem(fs)
            .resolve(resolve_options)
            .output(OutputOptionsBuilder::default()
                .path(context.join(dist_utf8))
                .filename(Filename::from("[name].js".to_string()))
                .public_path(PublicPath::from("".to_string()))
                .library(LibraryOptions {
                    name: None,
                    export: None,
                    umd_named_define: None,
                    auxiliary_comment: None,
                    amd_container: None,
                    library_type: LibraryType::from("commonjs2")
            }));



        let mut compiler = builder
            .build()
            .map_err(|e| anyhow!("Failed to build compiler: {:?}", e))?;
        
        compiler.build().await.map_err(|e| anyhow!("Build failed: {:?}", e))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    fn clean() {
        let dist = Path::new("tests/dist");
        if dist.exists() {
            std::fs::remove_dir_all(dist).unwrap();
        }
    }

    #[test]
    fn bundling_works() {
        clean();
        let targets = HashMap::from([("pages/home".to_owned(), "./tests/home.js".to_owned())]);

        match WebBundler::new(&targets, "tests/dist") {
            Ok(bundler) => {
                assert!(bundler.exec().is_ok());
                assert!(Path::new("tests/dist/pages/home.js").exists());
            }
            Err(err) => {
                panic!("BUNDLING TEST FAILED: {err:?}",)
            }
        }
        clean();
    }

    #[test]
    fn invalid_target_fails() {
        clean();
        let targets = HashMap::from([("invalid_path.tsx".to_owned(), "invalid_path".to_owned())]);
        let bundler = WebBundler::new(&targets, "tests/dist");
        assert!(bundler.is_err());
    }
}
