//! Writable sinks require exact receiver and destination authority.
use super::shapes;
use super::{Expr, Origin, Scanner, argument, named_path};
use shapes::{expression, profile, signature_valid};
use syn::{FnArg, Pat, Signature};
impl Scanner<'_> {
    pub(crate) fn check_root_receiver(&mut self, call: &syn::ExprMethodCall) {
        let canonical = matches!(
            self.name,
            "command_git_private_root.rs" | "command_git_private_root_files.rs"
        );
        if self.projection(&call.receiver)
            && !(canonical
                && call.args.is_empty()
                && matches!(call.method.to_string().as_str(), "metadata" | "display")
                || canonical
                    && self.owner == "PrivateGitRoot"
                    && self.function == "verify_binding"
                    && shapes::self_field(&call.receiver, "path")
                    && call.method == "join"
                    && call.args.len() == 1
                    && named_path(&call.args[0], &["name"]))
        {
            self.reject("uninspected method on protected root projection");
        }
    }

    pub(crate) fn check_root_binding(&mut self, local: &syn::Local) {
        if let Some(init) = &local.init {
            if self.projection(&init.expr)
                && !matches!(&local.pat, Pat::Ident(p) if p.by_ref.is_none() || p.mutability.is_none())
            {
                self.reject("unsupported protected-field binding pattern");
            }
        }
    }

    pub(super) fn projection(&self, expr: &Expr) -> bool {
        shapes::root_projection(
            expr,
            &self.origins,
            matches!(
                self.name,
                "command_git_private_root.rs" | "command_git_private_root_files.rs"
            ),
        )
    }

    pub(super) fn origin(&self, expr: &Expr) -> Origin {
        shapes::origin(expr, &self.origins, &self.function, self.trusted)
    }
    pub(crate) fn begin(&mut self, signature: &Signature, attrs: &[syn::Attribute]) {
        self.function = signature.ident.to_string();
        if !super::context::signature_attributes(signature, attrs) {
            self.reject("uninspected function attributes");
        }
        self.origins = self.flag_symbols.clone();
        self.verified = false;
        if self.owner == "PrivateGitRoot" {
            self.origins.insert("self".to_owned(), Origin::RootBorrow);
        }
        self.trusted = profile(self.name, &self.owner, &self.function)
            && signature_valid(&self.owner, signature);
        if profile(self.name, &self.owner, &self.function) && !self.trusted {
            self.reject("changed authority signature");
        }
        for arg in &signature.inputs {
            if let FnArg::Typed(arg) = arg {
                if shapes::root_type(&arg.ty) && !matches!(arg.pat.as_ref(), Pat::Ident(_)) {
                    self.reject("unsupported root parameter binding");
                }
                if let Pat::Ident(pat) = arg.pat.as_ref() {
                    let origin = if shapes::root_type(&arg.ty) {
                        Origin::RootBorrow
                    } else if shapes::formatter_type(&arg.ty) {
                        Origin::Formatter
                    } else {
                        Origin::Input
                    };
                    self.origins.insert(pat.ident.to_string(), origin);
                }
            }
        }
    }

    pub(crate) fn method_origin(&self, call: &syn::ExprMethodCall) -> Origin {
        shapes::method_origin(call, &self.origins, &self.function, self.trusted)
    }

    pub(crate) fn method_allowed(&self, call: &syn::ExprMethodCall) -> bool {
        if !self.trusted {
            return false;
        }
        match call.method.to_string().as_str() {
            "create" => {
                (self.verified
                    && self.function == "create_directory"
                    && self.origin(&call.receiver) == Origin::DirectoryBuilder
                    && argument(call, 0).is_some_and(|e| self.origin(e) == Origin::DirectorySlot))
                    || (self.function == "create_private_directory"
                        && self.origin(&call.receiver) == Origin::AllocBuilder
                        && argument(call, 0)
                            .is_some_and(|e| self.origin(e) == Origin::AllocDirectory))
            }
            "open" => {
                self.verified
                    && argument(call, 0).is_some_and(|e| self.origin(e) == Origin::FileSlot)
                    && ((self.function == "write_file"
                        && self.origin(&call.receiver) == Origin::WriteOptions)
                        || (self.function == "replace_contents"
                            && self.origin(&call.receiver) == Origin::ReplaceOptions
                            && argument(call, 0).is_some_and(|e| named_path(e, &["path"]))))
            }
            "write_all" => {
                self.verified
                    && argument(call, 0).is_some_and(|e| named_path(e, &["bytes"]))
                    && ((self.function == "write_file"
                        && self.origin(&call.receiver) == Origin::OwnedFileHandle)
                        || (self.function == "replace_contents"
                            && named_path(&call.receiver, &["handle"])
                            && self.origin(&call.receiver) == Origin::ExistingFileHandle))
            }
            "write" => {
                matches!(self.function.as_str(), "write_file" | "replace_contents")
                    && self.method_origin(call) == Origin::WriteEnabled
            }
            "create_new" => {
                self.function == "write_file"
                    && self.method_origin(call) == Origin::ExclusiveEnabled
            }
            "mode" => {
                matches!(
                    self.function.as_str(),
                    "write_file" | "create_directory" | "create_private_directory"
                ) && matches!(
                    self.method_origin(call),
                    Origin::WriteOptions | Origin::DirectoryBuilder | Origin::AllocBuilder
                )
            }
            "set_len" => {
                self.verified
                    && self.function == "replace_contents"
                    && named_path(&call.receiver, &["handle"])
                    && self.origin(&call.receiver) == Origin::ExistingFileHandle
                    && call.args.len() == 1
                    && matches!(&call.args[0], Expr::Lit(l) if matches!(&l.lit, syn::Lit::Int(i) if i.base10_parse::<u64>().ok() == Some(0)))
            }
            _ => false,
        }
    }

    pub(crate) fn call_allowed(&self, call: &syn::ExprCall) -> bool {
        if named_path(&call.func, &["Self", "create"])
            && call.args.len() == 1
            && self.owner == "PrivateGitRoot"
            && self.name == "command_git_private_root.rs"
            && matches!(
                self.function.as_str(),
                "create_repository" | "create_no_index"
            )
        {
            return true;
        }
        if !self.trusted {
            return false;
        }
        let Expr::Path(path) = call.func.as_ref() else {
            return false;
        };
        let names: Vec<_> = path
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        match names.join("::").as_str() {
            "fs::rename" => {
                self.verified
                    && self.function == "install_effective_config"
                    && call.args.len() == 2
                    && self.origin(&call.args[0]) == Origin::EffectiveConfig
                    && self.origin(&call.args[1]) == Origin::BootstrapConfig
            }
            "fs::remove_dir_all" => {
                self.verified
                    && self.function == "cleanup"
                    && call.args.len() == 1
                    && shapes::self_field(&call.args[0], "path")
            }
            _ => false,
        }
    }
}
