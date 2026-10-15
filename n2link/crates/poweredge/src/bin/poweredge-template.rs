//! `poweredge-template digest <template-dir>`  print the digest to pin in manifest.json
//! `poweredge-template verify <templates-root>` load every template and verify digests

use std::path::Path;
use std::process::ExitCode;

use poweredge::registry::compute_dir_digest;
use poweredge::TemplateRegistry;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match (args.get(1).map(String::as_str), args.get(2)) {
        (Some("digest"), Some(dir)) => match compute_dir_digest(Path::new(dir)) {
            Ok(digest) => {
                println!("{digest}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("error [{}]: {err}", err.code());
                ExitCode::FAILURE
            }
        },
        (Some("verify"), Some(root)) => match TemplateRegistry::load_dir(Path::new(root)) {
            Ok(registry) => {
                for template in registry.iter() {
                    println!("ok {}@{} {}", template.manifest.id, template.manifest.version, template.digest());
                }
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("error [{}]: {err}", err.code());
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("usage: poweredge-template digest <template-dir> | verify <templates-root>");
            ExitCode::from(2)
        }
    }
}
