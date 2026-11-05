#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! Conservative syntax guard for the deliberately small pure domain surface.
use std::{env, fs, path::Path, process::ExitCode};
use syn::{UseTree, visit::Visit};
const MODULES: &[&str] = &[
    "option",
    "result",
    "cmp",
    "clone",
    "fmt",
    "marker",
    "string",
    "vec",
    "boxed",
    "collections",
    "iter",
    "convert",
    "default",
    "mem",
    "primitive",
];
#[derive(Default)]
struct Purity {
    errors: Vec<String>,
}
impl Purity {
    fn check_segments(&mut self, segments: &[String]) {
        if let Some(root) = segments.first()
            && ["std", "core", "alloc"].contains(&root.as_str())
            && !segments
                .get(1)
                .is_some_and(|module| MODULES.contains(&module.as_str()))
        {
            self.errors
                .push(format!("Forbidden domain path: {}", segments.join("::")));
        }
    }
    fn check_use(&mut self, tree: &UseTree, mut prefix: Vec<String>) {
        match tree {
            UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.check_use(&path.tree, prefix);
            }
            UseTree::Name(name) => {
                prefix.push(name.ident.to_string());
                self.check_segments(&prefix);
            }
            UseTree::Rename(rename) => {
                prefix.push(rename.ident.to_string());
                self.check_segments(&prefix);
            }
            UseTree::Glob(_) => {
                self.errors.push("Domain glob imports are forbidden".into());
            }
            UseTree::Group(group) => {
                for item in &group.items {
                    self.check_use(item, prefix.clone());
                }
            }
        }
    }
}
impl<'ast> Visit<'ast> for Purity {
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        let test_only = module.content.is_some()
            && module.attrs.iter().any(|attribute| {
                attribute.path().is_ident("cfg")
                    && attribute
                        .parse_args::<syn::Path>()
                        .is_ok_and(|path| path.is_ident("test"))
            });
        if !test_only {
            syn::visit::visit_item_mod(self, module);
        }
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.check_segments(
            &path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>(),
        );
        syn::visit::visit_path(self, path);
    }
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.check_use(&item.tree, Vec::new());
    }
    fn visit_item_extern_crate(&mut self, _: &'ast syn::ItemExternCrate) {
        self.errors.push("Domain extern crate is forbidden".into());
    }
    fn visit_macro(&mut self, _: &'ast syn::Macro) {
        self.errors.push("Domain macros are forbidden".into());
    }
    fn visit_expr_unsafe(&mut self, _: &'ast syn::ExprUnsafe) {
        self.errors.push("Domain unsafe is forbidden".into());
    }
    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        if !attribute.path().is_ident("derive") && !attribute.path().is_ident("doc") {
            self.errors
                .push("Domain attributes permit only doc and built-in derives".into());
        }
        if attribute.path().is_ident("doc") {
            let literal = matches!(&attribute.meta, syn::Meta::NameValue(value) if matches!(&value.value, syn::Expr::Lit(literal) if matches!(literal.lit, syn::Lit::Str(_))));
            if !literal {
                self.errors
                    .push("Domain documentation must be a string literal".into());
            }
        }
        if attribute.path().is_ident("derive") {
            match attribute.parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            ) {
                Ok(paths) => {
                    for path in paths {
                        if path.segments.len() != 1
                            || ![
                                "Clone",
                                "Copy",
                                "Debug",
                                "Eq",
                                "PartialEq",
                                "Ord",
                                "PartialOrd",
                                "Hash",
                                "Default",
                            ]
                            .iter()
                            .any(|name| path.is_ident(name))
                        {
                            self.errors
                                .push("Only built-in domain derives are allowed".into());
                        }
                    }
                }
                Err(error) => self.errors.push(error.to_string()),
            }
        }
    }
}
fn scan(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            scan(&entry?.path())?;
        }
    } else if path.extension().is_some_and(|extension| extension == "rs") {
        let file = syn::parse_file(&fs::read_to_string(path)?)?;
        let mut purity = Purity::default();
        purity.visit_file(&file);
        if !purity.errors.is_empty() {
            return Err(format!("{}: {}", path.display(), purity.errors.join("; ")).into());
        }
    }
    Ok(())
}
fn main() -> ExitCode {
    let path = env::args()
        .nth(1)
        .unwrap_or_else(|| "crates/hf-domain/src".into());
    match scan(Path::new(&path)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
