use anyhow::{anyhow, Result};


// use lazy_static::lazy_static;
// use metacall::{load, metacall, MetaCallFuture, MetaCallValue};
// use metassr_utils::checker::CheckerState;
// use std::{
//     any::Any,
//     collections::HashMap,
//     ffi::OsStr,
//     marker::Sized,
//     path::Path,
//     sync::{Arc, Condvar, Mutex},
// };

// use tracing::error;
#[macro_use]
extern crate serde_json;

use std::{
    collections::HashMap,
    ffi::OsStr,
    marker::Sized,
    path::Path, 
    sync::Arc,
};

use rspack::builder::{Builder as _, Devtool, OutputOptionsBuilder};
use rspack_core::{Compiler, Experiments, Filename, PublicPath, LibraryOptions,
    LibraryType, Mode, ModuleOptions, ModuleRule, ModuleRuleEffect, ModuleRuleUse,
    ModuleRuleUseLoader, Resolve, RuleSetCondition};
use rspack_paths::Utf8Path;
use rspack_regex::RspackRegex;
use rspack_fs::{ WritableFileSystem, NativeFileSystem };

// lazy_static! {
//     /// A detector for if the bundling script `./bundle.js` is loaded or not. It is used to solve multiple loading script error in metacall.
//     static ref IS_BUNDLING_SCRIPT_LOADED: Mutex<CheckerState> = Mutex::new(CheckerState::default());

//     /// A simple checker to check if the bundling function is done or not. It is used to block the program until bundling done.
//     static ref IS_COMPILATION_WAIT: Arc<CompilationWait> = Arc::new(CompilationWait::default());
// }
// static BUILD_SCRIPT: &str = include_str!("./bundle.js");
// const BUNDLING_FUNC: &str = "web_bundling";

// /// A simple struct for compilation wait of the bundling function.
// struct CompilationWait {
//     checker: Mutex<CheckerState>,
//     cond: Condvar,
// }

// impl Default for CompilationWait {
//     fn default() -> Self {
//         Self {
//             checker: Mutex::new(CheckerState::default()),
//             cond: Condvar::new(),
//         }
//     }
// }

/// A web bundler that invokes the `web_bundling` function from the Node.js `bundle.js` script
/// using MetaCall. It is designed to bundle web resources like JavaScript and TypeScript files
/// by calling a custom `rspack` configuration.
///
/// The `exec` function blocks the execution until the bundling process completes.
#[derive(Debug)]
pub struct WebBundler<'a> {
    /// A map containing the source entry points for bundling.
    /// The key represents the entry name, and the value is the file path.
    pub targets: HashMap<String, &'a Path>,
    /// The output directory where the bundled files will be stored.
    pub dist_path: &'a Path,
}

impl<'a> WebBundler<'a> {
    /// Creates a new `WebBundler` instance.
    ///
    /// - `targets`: A HashMap where the key is a string representing an entry point, and the value is the file path.
    /// - `dist_path`: The path to the directory where the bundled output should be saved.
    ///
    /// Returns a `WebBundler` struct.
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

    /// Executes the bundling process by invoking the `web_bundling` function from `bundle.js` via MetaCall.
    ///
    /// It checks if the bundling script has been loaded, then calls the function and waits for the
    /// bundling to complete, either resolving successfully or logging an error.
    ///
    /// # Errors
    ///
    /// This function returns an `Err` if the bundling script cannot be loaded or if bundling fails.
    /// This function returns an `Err` if bundling fails.
    pub fn exec(&self) -> Result<()> {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                tokio::task::block_in_place(|| handle.block_on(async { self.exec_async().await }))
            }
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


        let js_rule = ModuleRule {
            test: Some(RuleSetCondition::Regexp(
                RspackRegex::new(r#"\.(jsx|js)$"#).unwrap(),
            )),
            effect: ModuleRuleEffect {
                r#use: ModuleRuleUse::Array(vec![ModuleRuleUseLoader {
                loader: "builtin:swc-loader".to_string(),
                options: Some(json!({
                    "jsc": {
                    "parser": {
                        "syntax": "ecmascript",
                        "jsx": true,
                    },
                    "transform": {
                        "react": {
                        "runtime": "automatic",
                        "pragma": "React.createElement",
                        "pragmaFrag": "React.Fragment",
                        "throwIfNamespace": true,
                        "useBuiltins": false
                        }
                    }
                    }
                }).to_string()),
                }]),
                ..Default::default()
            },
            ..Default::default()
        };
        
        let fs = Arc::new(NativeFileSystem::new(false));
        let dist_utf8 = Utf8Path::new(self.dist_path.to_str().ok_or_else(|| anyhow!("Invalid dist path"))?);

        fs.create_dir_all(dist_utf8).await
            .map_err(|e| anyhow!("Failed to create output directory: {:?}", e))?;


        builder.module(ModuleOptions::builder().rule(js_rule))
            .context(context)
            .experiments(Experiments::builder().css(true))
            .mode(Mode::Production)
            .devtool(Devtool::SourceMap)
            .enable_loader_swc()
            // .enable_loader_react_refresh()
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
                    library_type: "commonjs2".to_string() as LibraryType
            }));



        let mut compiler = builder
            .build()
            .map_err(|e| anyhow!("Failed to build compiler: {:?}", e))?;
        
        compiler.build().await.map_err(|e| anyhow!("Build failed: {:?}", e))?;

        // let errors: Vec<_> = compiler.compilation.get_errors().collect();
        // if !errors.is_empty() {
        //     println!("{:#?}", errors);
        //     return Err(anyhow!("Compilation errors: {:?}", errors));
        // }
        Ok(())
    }

    // /// Executes the bundling process by invoking the `web_bundling` function from `bundle.js` via MetaCall.
    // ///
    // /// It checks if the bundling script has been loaded, then calls the function and waits for the
    // /// bundling to complete, either resolving successfully or logging an error.
    // ///
    // /// # Errors
    // ///
    // /// This function returns an `Err` if the bundling script cannot be loaded or if bundling fails.
    // pub fn exec(&self) -> Result<()> {
    //     // Lock the mutex to check if the bundling script is already loaded
    //     let mut guard = IS_BUNDLING_SCRIPT_LOADED.lock().unwrap();
    //     if !guard.is_true() {
    //         // If not loaded, attempt to load the script into MetaCall
    //         // println!("{:?}", BUILD_SCRIPT);
    //         if let Err(e) = load::from_memory(load::Tag::NodeJS, BUILD_SCRIPT, None) {
    //             return Err(anyhow!("Cannot load bundling script: {e:?}"));
    //         }
    //         // Mark the script as loaded
    //         guard.make_true();
    //     }
    //     // Drop the lock on the mutex as it's no longer needed
    //     drop(guard);

    //     // Resolve callback when the bundling process is completed successfully
    //     fn resolve(
    //         result: Box<dyn MetaCallValue>,
    //         _: Option<Box<dyn Any>>,
    //     ) -> Box<dyn MetaCallValue> {
    //         let compilation_wait = &*Arc::clone(&IS_COMPILATION_WAIT);
    //         let mut started = compilation_wait.checker.lock().unwrap();

    //         // Mark the process as completed and notify waiting threads
    //         started.make_true();
    //         compilation_wait.cond.notify_one();

    //         result
    //     }

    //     // Reject callback for handling errors during the bundling process
    //     fn reject(err: Box<dyn MetaCallValue>, _: Option<Box<dyn Any>>) -> Box<dyn MetaCallValue> {
    //         let compilation_wait = &*Arc::clone(&IS_COMPILATION_WAIT);
    //         let mut started = compilation_wait.checker.lock().unwrap();

    //         // Log the bundling error and mark the process as completed
    //         error!("Bundling rejected: {err:?}");
    //         started.make_true();
    //         compilation_wait.cond.notify_one();

    //         err
    //     }

    //     // Call the `web_bundling` function in the MetaCall script with targets and output path
    //     let future = metacall::<MetaCallFuture>(
    //         BUNDLING_FUNC,
    //         [
    //             // Serialize the targets map to a string format
    //             serde_json::to_string(&self.targets)?,
    //             // Get the distribution path as a string
    //             self.dist_path.to_str().unwrap().to_owned(),
    //         ],
    //     )
    //     .unwrap();

    //     // Set the resolve and reject handlers for the bundling future
    //     future.then(resolve).catch(reject).await_fut();

    //     // Lock the mutex and wait for the bundling process to complete
    //     let compilation_wait = Arc::clone(&IS_COMPILATION_WAIT);
    //     let mut started = compilation_wait.checker.lock().unwrap();

    //     // Block the current thread until the bundling process signals completion
    //     while !started.is_true() {
    //         started = Arc::clone(&IS_COMPILATION_WAIT).cond.wait(started).unwrap();
    //     }

    //     // Reset the checker state to false after the process completes
    //     started.make_false();
    //     Ok(())
    // }
}

#[cfg(test)]
mod tests {

    use super::*;
    // use metacall::initialize;

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
