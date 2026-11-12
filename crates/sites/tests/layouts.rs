//! Every field of a C layout has a constant offset assertion, including padding and opaque storage.

use std::collections::BTreeSet;
use std::path::Path;

use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{BinOp, Expr, ItemConst, ItemStruct, Lit, Macro, Member, Meta, Token, Type};

#[derive(Default)]
struct Layouts {
    fields: BTreeSet<(String, String)>,
    pinned: BTreeSet<(String, String)>,
    const_depth: usize,
}

struct Offset {
    ty: Type,
    member: Member,
}

impl Parse for Offset {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let ty = input.parse()?;
        input.parse::<Token![,]>()?;
        let member = input.parse()?;
        Ok(Self { ty, member })
    }
}

fn member_name(member: &Member) -> String {
    match member {
        Member::Named(name) => name.to_string(),
        Member::Unnamed(index) => index.index.to_string(),
    }
}

fn is_macro(mac: &Macro, name: &str) -> bool {
    mac.path.segments.last().is_some_and(|s| s.ident == name)
}

impl<'ast> Visit<'ast> for Layouts {
    fn visit_item_struct(&mut self, item: &'ast ItemStruct) {
        let repr_c = item.attrs.iter().any(|attr| {
            attr.path().is_ident("repr")
                && attr
                    .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                    .is_ok_and(|args| args.iter().any(|arg| arg.path().is_ident("C")))
        });
        if repr_c {
            for (i, field) in item.fields.iter().enumerate() {
                let name = field
                    .ident
                    .as_ref()
                    .map_or_else(|| i.to_string(), ToString::to_string);
                self.fields.insert((item.ident.to_string(), name));
            }
        }
        visit::visit_item_struct(self, item);
    }

    fn visit_item_const(&mut self, item: &'ast ItemConst) {
        self.const_depth += 1;
        visit::visit_item_const(self, item);
        self.const_depth -= 1;
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        if self.const_depth == 0 || !is_macro(mac, "assert") {
            return;
        }
        let Ok(Expr::Binary(expr)) = syn::parse2::<Expr>(mac.tokens.clone()) else {
            return;
        };
        if !matches!(expr.op, BinOp::Eq(_)) {
            return;
        }
        let (Expr::Macro(offset), Expr::Lit(value)) = (*expr.left, *expr.right) else {
            return;
        };
        if !is_macro(&offset.mac, "offset_of") || !matches!(value.lit, Lit::Int(_)) {
            return;
        }
        let Ok(Offset {
            ty: Type::Path(ty),
            member,
        }) = syn::parse2(offset.mac.tokens)
        else {
            return;
        };
        if let Some(name) = ty.path.segments.last() {
            self.pinned
                .insert((name.ident.to_string(), member_name(&member)));
        }
    }
}

fn missing(src: &str) -> Vec<String> {
    let file = syn::parse_file(src).unwrap_or_else(|e| panic!("invalid Rust source: {e}"));
    let mut layouts = Layouts::default();
    layouts.visit_file(&file);
    layouts
        .fields
        .difference(&layouts.pinned)
        .map(|(ty, field)| format!("{ty}.{field}"))
        .collect()
}

fn scan(dir: &Path, failures: &mut Vec<String>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .path();
        if path.is_dir() {
            scan(&path, failures);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let src = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            failures.extend(
                missing(&src)
                    .into_iter()
                    .map(|field| format!("{}: {field}", path.display())),
            );
        }
    }
}

#[test]
fn every_c_layout_field_has_a_const_offset_assertion() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let entries =
        std::fs::read_dir(&crates).unwrap_or_else(|e| panic!("{}: {e}", crates.display()));
    let mut failures = Vec::new();
    for entry in entries {
        let src = entry
            .unwrap_or_else(|e| panic!("{}: {e}", crates.display()))
            .path()
            .join("src");
        if src.is_dir() {
            scan(&src, &mut failures);
        }
    }
    failures.sort();
    assert!(
        failures.is_empty(),
        "missing constant offset assertions:\n{}",
        failures.join("\n")
    );
}

#[test]
fn the_guard_requires_assertions_in_constants() {
    assert_eq!(
        missing(
            r#"
            #[repr(C)] struct Layout { first: u32, _pad: [u8; 4], last: u32 }
            // assert!(offset_of!(Layout, _pad) == 4);
            const TEXT: &str = "assert!(offset_of!(Layout, _pad) == 4)";
            const _: () = { assert!(core::mem::offset_of!(Layout, first) == 0); };
            fn check() { assert!(offset_of!(Layout, last) == 8); }
            "#
        ),
        ["Layout._pad", "Layout.last"]
    );
}

#[test]
fn the_guard_handles_generics_tuples_and_transparent_wrappers() {
    assert!(
        missing(
            r#"
        #[repr(transparent)] struct Wrapper { field: u32 }
        #[repr(C, align(8))] struct Layout<T> { data: *mut T }
        #[repr(C)] struct Storage([u8; 8]);
        const _: () = {
            assert!(offset_of!(Layout<u8>, data) == 0);
            assert!(offset_of!(Storage, 0) == 0);
        };
        "#
        )
        .is_empty()
    );
}
