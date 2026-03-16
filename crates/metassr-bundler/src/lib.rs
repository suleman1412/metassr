use anyhow::{anyhow, Result};
use serde_json::json;
use std::{
    collections::HashMap,
    ffi::OsStr,
    marker::Sized,
    path::Path, 
    sync::Arc, vec,
};
use rspack::builder::{Builder as _, Devtool, OptimizationOptionsBuilder, OutputOptionsBuilder, ModuleOptionsBuilder};
use rspack_core::{Compiler, Experiments, Filename, PublicPath, LibraryOptions,
    LibraryType, Mode, ModuleRule, ModuleRuleEffect, ModuleRuleUse,
    ModuleRuleUseLoader, Resolve, RuleSetCondition, ModuleType, EntryDescription, 
    StatsOptions};
use rspack_paths::Utf8PathBuf;
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
        let mut compiler = Compiler::builder();
        for (entry_name, entry_path) in &self.targets {
            let entry_path_str = entry_path
                .to_str()
                .ok_or_else(|| anyhow!("Invalid UTF-8 in entry path: {:?}", entry_path))?;
            
            compiler.entry(
                entry_name.clone(),
                EntryDescription::from(entry_path_str)
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use metacall::initialize;

    fn clean() {
        let dist = Path::new("tests/dist");
        if dist.exists() {
            std::fs::remove_dir_all(dist).unwrap();
        }
    }

    #[test]
    fn bundling_works() {
        clean();
        let _metacall = initialize().unwrap();
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
