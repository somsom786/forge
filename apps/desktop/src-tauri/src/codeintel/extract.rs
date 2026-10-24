//! Symbol extraction from tree-sitter syntax trees.
//!
//! Parsing only: the grammars are compiled-in C parse tables, nothing from the repository is executed, and a deadline in
//! the parser's progress callback stops a pathological file instead of hanging the index. Only declarations are walked
//! (module level, class/impl/trait bodies, namespaces), never function bodies, so locals do not show up as symbols.

use super::{Lang, Symbol};
use std::ops::ControlFlow;
use std::time::{Duration, Instant};
use tree_sitter::{Node, ParseOptions, Parser};

/// Per-file parse deadline. Real source files of the size we accept parse in milliseconds.
pub const PARSE_DEADLINE: Duration = Duration::from_secs(2);
const SIGNATURE_CHARS: usize = 160;
const MAX_DEPTH: usize = 12;

fn language(lang: Lang) -> tree_sitter::Language {
    match lang {
        Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
        Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        Lang::Python => tree_sitter_python::LANGUAGE.into(),
    }
}

pub struct Extracted {
    pub symbols: Vec<Symbol>,
    /// The tree contains ERROR/MISSING nodes: symbols were still extracted from the parts that parsed.
    pub had_errors: bool,
}

/// Parse one file and list its declarations. `Err` only when the grammar could not be loaded or the deadline passed.
pub fn extract(parser: &mut Parser, lang: Lang, file: &str, src: &[u8]) -> Result<Extracted, String> {
    parser.set_language(&language(lang)).map_err(|e| format!("grammar could not be loaded: {e}"))?;
    let started = Instant::now();
    let mut progress = |_: &tree_sitter::ParseState| if started.elapsed() > PARSE_DEADLINE { ControlFlow::Break(()) } else { ControlFlow::Continue(()) };
    let len = src.len();
    let tree = parser.parse_with_options(&mut |i: usize, _| if i < len { &src[i..] } else { &[] as &[u8] }, None, Some(ParseOptions::new().progress_callback(&mut progress)));
    let Some(tree) = tree else {
        parser.reset();
        return Err(format!("parsing took longer than {} s", PARSE_DEADLINE.as_secs()));
    };
    let root = tree.root_node();
    let mut w = Walker { src, file, out: vec![] };
    match lang {
        Lang::Rust => w.rust(root, None, false, 0),
        Lang::TypeScript | Lang::Tsx | Lang::JavaScript => w.script(root, None, false, 0),
        Lang::Python => w.python(root, None, false, 0),
    }
    Ok(Extracted { symbols: w.out, had_errors: root.has_error() })
}

struct Walker<'a> {
    src: &'a [u8],
    file: &'a str,
    out: Vec<Symbol>,
}

fn children(n: Node<'_>) -> Vec<Node<'_>> {
    let mut c = n.walk();
    n.named_children(&mut c).collect()
}

impl<'a> Walker<'a> {
    fn text(&self, n: Node) -> &'a str {
        n.utf8_text(self.src).unwrap_or("")
    }

    fn name_of(&self, n: Node) -> Option<String> {
        let name = self.text(n.child_by_field_name("name")?).trim();
        (!name.is_empty() && name.chars().count() <= 120).then(|| name.to_string())
    }

    /// The declaration up to its body, on one line: `pub fn refresh(&self, t: Token) -> Result<()>`.
    fn signature(&self, n: Node) -> String {
        let end = n.child_by_field_name("body").map(|b| b.start_byte()).unwrap_or(n.end_byte()).max(n.start_byte());
        let raw = String::from_utf8_lossy(&self.src[n.start_byte()..end.min(self.src.len())]);
        let one: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        let one = one.trim_end_matches(['{', ':', ' ']).to_string();
        if one.chars().count() > SIGNATURE_CHARS { one.chars().take(SIGNATURE_CHARS).collect::<String>() + "…" } else { one }
    }

    fn push(&mut self, n: Node, sig_node: Node, name: String, kind: &'static str, container: Option<&str>) {
        self.out.push(Symbol {
            name,
            kind,
            file: self.file.to_string(),
            start_line: n.start_position().row as u32 + 1,
            end_line: n.end_position().row as u32 + 1,
            signature: self.signature(sig_node),
            container: container.map(str::to_string),
        });
    }

    fn rust(&mut self, node: Node, container: Option<&str>, in_impl: bool, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        for ch in children(node) {
            let simple = match ch.kind() {
                "function_item" | "function_signature_item" => Some(if in_impl { "method" } else { "function" }),
                "struct_item" | "union_item" => Some("struct"),
                "enum_item" => Some("enum"),
                "type_item" => Some("type"),
                "const_item" | "static_item" => Some("const"),
                _ => None,
            };
            if let Some(kind) = simple {
                if let Some(name) = self.name_of(ch) {
                    self.push(ch, ch, name, kind, container);
                }
                continue;
            }
            match ch.kind() {
                "trait_item" | "mod_item" => {
                    let Some(name) = self.name_of(ch) else { continue };
                    let kind = if ch.kind() == "trait_item" { "trait" } else { "module" };
                    self.push(ch, ch, name.clone(), kind, container);
                    if let Some(body) = ch.child_by_field_name("body") {
                        let inner = qualify(container, &name);
                        self.rust(body, Some(&inner), kind == "trait", depth + 1);
                    }
                }
                // `impl Foo` / `impl Trait for Foo`: not a symbol itself; its functions are methods of `Foo`.
                "impl_item" => {
                    let ty = ch.child_by_field_name("type").map(|t| base_type_name(self.text(t))).unwrap_or_default();
                    if let Some(body) = ch.child_by_field_name("body") {
                        let inner = qualify(container, &ty);
                        self.rust(body, Some(&inner), true, depth + 1);
                    }
                }
                _ => {}
            }
        }
    }

    /// TypeScript, TSX and JavaScript share most node kinds.
    fn script(&mut self, node: Node, container: Option<&str>, in_class: bool, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        for ch in children(node) {
            match ch.kind() {
                // A top-level `namespace X {}` parses as an expression statement wrapping the module.
                "export_statement" | "ambient_declaration" | "expression_statement" => self.script(ch, container, in_class, depth + 1),
                "function_declaration" | "generator_function_declaration" | "function_signature" => {
                    if let Some(name) = self.name_of(ch) {
                        self.push(ch, ch, name, "function", container);
                    }
                }
                "class_declaration" | "abstract_class_declaration" => {
                    let Some(name) = self.name_of(ch) else { continue };
                    self.push(ch, ch, name.clone(), "class", container);
                    if let Some(body) = ch.child_by_field_name("body") {
                        self.script(body, Some(&qualify_dot(container, &name)), true, depth + 1);
                    }
                }
                "method_definition" | "abstract_method_signature" if in_class => {
                    if let Some(name) = self.name_of(ch) {
                        self.push(ch, ch, name, "method", container);
                    }
                }
                "public_field_definition" | "field_definition" if in_class => {
                    // `handle = (e) => {...}` class fields are methods in all but syntax.
                    let value = ch.child_by_field_name("value");
                    if value.is_some_and(|v| is_function_value(v.kind())) {
                        let name = ch.child_by_field_name("name").or_else(|| ch.child_by_field_name("property")).map(|n| self.text(n).to_string());
                        if let Some(name) = name.filter(|n| !n.is_empty() && n.chars().count() <= 120) {
                            self.push(ch, ch, name, "method", container);
                        }
                    }
                }
                "interface_declaration" => {
                    if let Some(name) = self.name_of(ch) {
                        self.push(ch, ch, name, "interface", container);
                    }
                }
                "type_alias_declaration" => {
                    if let Some(name) = self.name_of(ch) {
                        self.push(ch, ch, name, "type", container);
                    }
                }
                "enum_declaration" => {
                    if let Some(name) = self.name_of(ch) {
                        self.push(ch, ch, name, "enum", container);
                    }
                }
                "internal_module" | "module" => {
                    let Some(name) = self.name_of(ch) else { continue };
                    let name = name.trim_matches(['"', '\'']).to_string();
                    self.push(ch, ch, name.clone(), "module", container);
                    if let Some(body) = ch.child_by_field_name("body") {
                        self.script(body, Some(&qualify_dot(container, &name)), false, depth + 1);
                    }
                }
                // CommonJS and prototype style: `res.send = function send(body) {…}`, `exports.parse = (s) => …`,
                // `Router.prototype.route = function (p) {…}`, `module.exports = function compile() {…}`.
                "assignment_expression" if !in_class => {
                    let (Some(left), Some(value)) = (ch.child_by_field_name("left"), ch.child_by_field_name("right")) else { continue };
                    let kind = match value.kind() {
                        k if is_function_value(k) => "function",
                        "class" => "class",
                        // `a = b = function …`: the innermost link is the one that names it.
                        "assignment_expression" => {
                            self.script(ch, container, in_class, depth + 1);
                            continue;
                        }
                        _ => continue,
                    };
                    let (name, owner) = match left.kind() {
                        "member_expression" => {
                            let prop = left.child_by_field_name("property").map(|n| self.text(n).to_string()).unwrap_or_default();
                            let object = left.child_by_field_name("object").map(|n| self.text(n).to_string()).unwrap_or_default();
                            let owner = object.trim_end_matches(".prototype").to_string();
                            if prop == "exports" && object == "module" {
                                // The module itself: named after the function, if it has a name.
                                match value.child_by_field_name("name") {
                                    Some(n) => (self.text(n).to_string(), None),
                                    None => continue,
                                }
                            } else {
                                let owner = (!matches!(owner.as_str(), "exports" | "module.exports" | "this") && owner.chars().count() <= 60 && !owner.contains(['(', '[', '\n'])).then_some(owner);
                                (prop, owner)
                            }
                        }
                        "identifier" => (self.text(left).to_string(), None),
                        _ => continue,
                    };
                    if name.is_empty() || name.chars().count() > 120 || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                        continue;
                    }
                    let owned = owner.map(|o| qualify_dot(container, &o));
                    let kind = if kind == "function" && owned.is_some() { "method" } else { kind };
                    self.push(ch, value, name, kind, owned.as_deref().or(container));
                }
                "lexical_declaration" | "variable_declaration" if !in_class => {
                    let is_const = self.text(ch).trim_start().starts_with("const");
                    for d in children(ch).into_iter().filter(|d| d.kind() == "variable_declarator") {
                        let Some(name_node) = d.child_by_field_name("name").filter(|n| n.kind() == "identifier") else { continue };
                        let name = self.text(name_node).to_string();
                        let kind = match d.child_by_field_name("value").map(|v| v.kind()) {
                            Some(k) if is_function_value(k) => "function",
                            Some("class") => "class",
                            _ if is_const => "const",
                            _ => continue,
                        };
                        // Line span and signature of the whole declaration (`export const x = ...`), named by the declarator.
                        self.push(ch, ch, name, kind, container);
                    }
                }
                _ => {}
            }
        }
    }

    fn python(&mut self, node: Node, container: Option<&str>, in_class: bool, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        for ch in children(node) {
            let (def, outer) = if ch.kind() == "decorated_definition" {
                match ch.child_by_field_name("definition") {
                    Some(d) => (d, ch),
                    None => continue,
                }
            } else {
                (ch, ch)
            };
            match def.kind() {
                "function_definition" => {
                    if let Some(name) = self.name_of(def) {
                        self.push(outer, def, name, if in_class { "method" } else { "function" }, container);
                    }
                }
                "class_definition" => {
                    let Some(name) = self.name_of(def) else { continue };
                    self.push(outer, def, name.clone(), "class", container);
                    if let Some(body) = def.child_by_field_name("body") {
                        self.python(body, Some(&qualify_dot(container, &name)), true, depth + 1);
                    }
                }
                // Module-level `MAX_RETRIES = 5`: by convention a constant.
                "expression_statement" if container.is_none() => {
                    for a in children(def).into_iter().filter(|a| a.kind() == "assignment") {
                        let Some(left) = a.child_by_field_name("left").filter(|l| l.kind() == "identifier") else { continue };
                        let name = self.text(left);
                        if name.len() > 1 && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') && name.chars().any(|c| c.is_ascii_alphabetic()) {
                            self.push(def, def, name.to_string(), "const", container);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn is_function_value(kind: &str) -> bool {
    matches!(kind, "arrow_function" | "function_expression" | "function" | "generator_function")
}

/// Rust paths use `::`; the others `.`.
fn qualify(container: Option<&str>, name: &str) -> String {
    qualify_with(container, name, "::")
}

fn qualify_dot(container: Option<&str>, name: &str) -> String {
    qualify_with(container, name, ".")
}

fn qualify_with(container: Option<&str>, name: &str, sep: &str) -> String {
    match container {
        Some(c) if !c.is_empty() => format!("{c}{sep}{name}"),
        _ => name.to_string(),
    }
}

/// `Foo<T>` / `crate::a::Foo<'a>` / `&mut Foo` -> `Foo`.
fn base_type_name(t: &str) -> String {
    let t = t.split('<').next().unwrap_or(t);
    let t = t.rsplit("::").next().unwrap_or(t);
    t.trim_start_matches(['&', '*']).trim_start_matches("mut ").trim_start_matches("dyn ").trim().to_string()
}
