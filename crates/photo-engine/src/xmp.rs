//! XMP: the rating, colour label and keywords other photo applications read and write.
//!
//! Three uses:
//! - reading what Lightroom, darktable, Capture One or a camera left — an `.xmp` sidecar
//!   beside the photo, or a packet inside it — to seed a photo Duckfoot has no sidecar for
//!   ([`seed_metadata`]);
//! - embedding the same in an export, so a DAM that opens the file sees them ([`packet`]);
//! - keeping an `.xmp` sidecar that already exists in step with Duckfoot's marks, without
//!   disturbing anything else in it ([`merge`]).
//!
//! The parser and the merge are adapted from LightCraft's `crates/meta` (`xmp.rs`,
//! `xmp_merge.rs`, at storytold/lightcraft b3d392b4), used under its MIT licence:
//! Copyright (c) 2026 ArtCraft Team and the LightCraft contributors (see
//! THIRD_PARTY_LICENSES.md). Changed here: LightCraft's metadata model, struct values and
//! regions are dropped, the namespaces are Duckfoot's (darktable's added), and the code is
//! written for edition 2021.

use crate::sidecar::{AssetMetadata, Flag};
use quick_xml::events::Event;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

const NAMESPACES: &[(&str, &str)] = &[
    ("x", "adobe:ns:meta/"),
    ("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
    ("dc", "http://purl.org/dc/elements/1.1/"),
    ("xmp", "http://ns.adobe.com/xap/1.0/"),
    ("xmpRights", "http://ns.adobe.com/xap/1.0/rights/"),
    ("photoshop", "http://ns.adobe.com/photoshop/1.0/"),
    ("lr", "http://ns.adobe.com/lightroom/1.0/"),
    ("darktable", "http://darktable.sf.net/"),
];

/// The colour labels Duckfoot has, as Lightroom names them in `xmp:Label` and in the
/// order darktable numbers them in `darktable:colorlabels`.
const LABELS: [(&str, &str); 5] = [
    ("red", "Red"),
    ("yellow", "Yellow"),
    ("green", "Green"),
    ("blue", "Blue"),
    ("purple", "Purple"),
];

/// What a read or a merge found wrong with a packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmpError {
    Xml(String),
    /// Nesting or size beyond any real packet's.
    TooComplex,
}

impl std::fmt::Display for XmpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XmpError::Xml(e) => write!(f, "XMP is not well-formed XML: {e}"),
            XmpError::TooComplex => write!(f, "XMP packet too deeply nested or too large"),
        }
    }
}

impl std::error::Error for XmpError {}

/// Every property of a packet, keyed `prefix:name` with canonical prefixes (struct fields
/// as `prefix:name/prefix:field`); arrays keep their order, language alternatives put
/// `x-default` first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Xmp {
    pub properties: BTreeMap<String, Vec<String>>,
}

impl Xmp {
    fn first(&self, key: &str) -> Option<&str> {
        self.properties
            .get(key)
            .and_then(|v| v.first())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
    }

    /// The marks this packet holds, as Duckfoot keeps them. A packet that states none
    /// gives `None`, so it cannot pass for "unrated, no keywords".
    pub fn marks(&self) -> Option<AssetMetadata> {
        let mut m = AssetMetadata::default();
        let mut any = false;
        // xmp:Rating is a real number; -1 means rejected (Lightroom, darktable, Bridge).
        if let Some(r) = self.first("xmp:Rating").and_then(|s| s.parse::<f32>().ok()) {
            any = true;
            if r < 0.0 {
                m.flag = Flag::Reject;
            } else {
                m.rating = r.round().clamp(0.0, 5.0) as u8;
            }
        }
        let label = self
            .first("xmp:Label")
            .and_then(|l| {
                LABELS
                    .iter()
                    .find(|(_, name)| name.eq_ignore_ascii_case(l))
                    .map(|(ours, _)| *ours)
            })
            .or_else(|| {
                // darktable numbers its labels, any number of them; the first is kept.
                self.properties
                    .get("darktable:colorlabels")
                    .and_then(|v| v.iter().find_map(|n| n.trim().parse::<usize>().ok()))
                    .and_then(|i| LABELS.get(i))
                    .map(|(ours, _)| *ours)
            });
        if let Some(label) = label {
            any = true;
            m.color_label = Some(label.to_string());
        }
        if let Some(words) = self.properties.get("dc:subject") {
            any |= !words.is_empty();
            m.add_keywords(&words.join(","));
        }
        any.then_some(m)
    }
}

/// Parse an XMP packet (with or without the `<?xpacket?>` wrapper or `x:xmpmeta`).
pub fn parse(s: &str) -> Result<Xmp, XmpError> {
    let s = s.trim_start_matches('\u{feff}');
    let tree = parse_tree(s)?;
    let mut descs = Vec::new();
    find_descriptions(&tree, &mut descs);
    let mut properties = BTreeMap::new();
    for d in descs {
        collect_description(d, "", &mut properties);
    }
    Ok(Xmp { properties })
}

/// The sidecars other applications keep beside `photo`: darktable's `IMG_1.CR3.xmp` and
/// Lightroom's `IMG_1.xmp`, in that order.
pub fn sidecar_paths(photo: &Path) -> [PathBuf; 2] {
    let mut full = photo.as_os_str().to_owned();
    full.push(".xmp");
    [PathBuf::from(full), photo.with_extension("xmp")]
}

/// How much of a photo is searched for an embedded packet. JPEG, PNG, WebP and TIFF keep
/// theirs near the start; a raw's is usually there too, and one that is not is skipped
/// rather than reading a 100 MB file.
const EMBEDDED_SEARCH: u64 = 1 << 20;

/// The marks other applications left for `photo`: its `.xmp` sidecar if it has one that
/// says something, else a packet inside the file. For a photo with no Duckfoot sidecar.
pub fn seed_metadata(photo: &Path) -> Option<AssetMetadata> {
    for path in sidecar_paths(photo) {
        if path == photo {
            continue;
        }
        if let Some(m) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| parse(&s).ok())
            .and_then(|x| x.marks())
        {
            return Some(m);
        }
    }
    embedded(photo).and_then(|x| x.marks())
}

/// A packet inside the first megabyte of `photo`, found by its `x:xmpmeta` element, which
/// every container holds as plain text.
fn embedded(photo: &Path) -> Option<Xmp> {
    use std::io::Read;
    let mut head = Vec::new();
    std::fs::File::open(photo)
        .ok()?
        .take(EMBEDDED_SEARCH)
        .read_to_end(&mut head)
        .ok()?;
    find_packet(&head)
}

/// The first packet in `bytes`, by its `x:xmpmeta` element.
pub(crate) fn find_packet(bytes: &[u8]) -> Option<Xmp> {
    let start = memchr::memmem::find(bytes, b"<x:xmpmeta")?;
    let len = memchr::memmem::find(&bytes[start..], b"</x:xmpmeta>")?;
    let text = std::str::from_utf8(&bytes[start..start + len + b"</x:xmpmeta>".len()]).ok()?;
    parse(text).ok()
}

/// What an export says about the photo beyond its EXIF.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExportXmp<'a> {
    pub marks: Option<&'a AssetMetadata>,
    pub creator: Option<&'a str>,
    pub rights: Option<&'a str>,
}

impl ExportXmp<'_> {
    fn is_empty(&self) -> bool {
        self.creator.is_none()
            && self.rights.is_none()
            && self.marks.is_none_or(|m| {
                m.rating == 0
                    && m.flag != Flag::Reject
                    && m.color_label.is_none()
                    && m.keywords.is_empty()
            })
    }
}

/// A packet holding `what`, or `None` when there is nothing to say. Rating and label are
/// attributes, as Lightroom writes them; keywords a `dc:subject` bag; creator a
/// `dc:creator` sequence and rights a `dc:rights` alternative, as XMP defines them.
pub fn packet(what: &ExportXmp) -> Option<String> {
    (!what.is_empty()).then(|| render(what))
}

/// The packet for `what`, even when it says nothing.
fn render(what: &ExportXmp) -> String {
    let esc = |s: &str| quick_xml::escape::escape(s).into_owned();
    let mut attrs = String::new();
    let mut body = String::new();
    if let Some(m) = what.marks {
        let rating = if m.flag == Flag::Reject {
            -1
        } else {
            i32::from(m.rating.min(5))
        };
        if rating != 0 {
            attrs.push_str(&format!("\n    xmp:Rating=\"{rating}\""));
        }
        if let Some((_, name)) = m
            .color_label
            .as_deref()
            .and_then(|l| LABELS.iter().find(|(ours, _)| *ours == l))
        {
            attrs.push_str(&format!("\n    xmp:Label=\"{name}\""));
        }
        if !m.keywords.is_empty() {
            body.push_str("\n   <dc:subject>\n    <rdf:Bag>");
            for k in &m.keywords {
                body.push_str(&format!("\n     <rdf:li>{}</rdf:li>", esc(k)));
            }
            body.push_str("\n    </rdf:Bag>\n   </dc:subject>");
        }
    }
    if let Some(c) = what.creator {
        body.push_str(&format!(
            "\n   <dc:creator>\n    <rdf:Seq>\n     <rdf:li>{}</rdf:li>\n    </rdf:Seq>\n   </dc:creator>",
            esc(c)
        ));
    }
    if let Some(r) = what.rights {
        body.push_str(&format!(
            "\n   <dc:rights>\n    <rdf:Alt>\n     <rdf:li xml:lang=\"x-default\">{}</rdf:li>\n    </rdf:Alt>\n   </dc:rights>",
            esc(r)
        ));
    }
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n \
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  \
         <rdf:Description rdf:about=\"\"\n    \
         xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"\n    \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\"{attrs}>{body}\n  \
         </rdf:Description>\n \
         </rdf:RDF>\n\
         </x:xmpmeta>\n\
         <?xpacket end=\"w\"?>"
    )
}

/// The properties Duckfoot's marks are kept in, which [`merge`] replaces. darktable's own
/// label list is replaced too, so the two cannot disagree.
const OWNED: &[&str] = &[
    "xmp:Rating",
    "xmp:Label",
    "dc:subject",
    "darktable:colorlabels",
];

/// `existing` (another application's sidecar) with Duckfoot's `marks` in place of the
/// rating, label and keywords it held, and everything else in it — develop settings,
/// history, other namespaces, comments — byte for byte as it was. Fails, and the caller
/// must then leave the file alone, when `existing` is not a well-formed packet.
pub fn merge(existing: &str, marks: &AssetMetadata) -> Result<String, XmpError> {
    // With no marks the packet is empty, and the merge then clears them from the file.
    let fresh = render(&ExportXmp {
        marks: Some(marks),
        ..Default::default()
    });
    merge_packets(
        existing,
        &fresh,
        MergeRules {
            owned: OWNED,
            owned_if_present: &[],
        },
    )
}

/// Bring the `.xmp` sidecar beside `photo`, if it has one, in step with `marks`. A photo
/// with none is left without one: Duckfoot's own sidecar is where its marks live.
pub fn update_sidecar(photo: &Path, marks: &AssetMetadata) -> anyhow::Result<Option<PathBuf>> {
    for path in sidecar_paths(photo) {
        if path == photo || !path.is_file() {
            continue;
        }
        let existing = std::fs::read_to_string(&path)?;
        let merged =
            merge(&existing, marks).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        if merged != existing {
            crate::fsio::atomic_write_bytes(&path, merged.as_bytes())?;
        }
        return Ok(Some(path));
    }
    Ok(None)
}

// ── The parser (after LightCraft's `xmp.rs`) ─────────────────────────────────────────

#[derive(Debug, Default)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn attr(&self, n: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, v)| v.as_str())
    }
}

const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 200_000;

/// `prefix:local` with the prefix canonicalised by namespace URI (see [`NAMESPACES`]), so a
/// packet that binds Dublin Core to `foo:` still yields `dc:subject`.
fn canonical(qname: &str, scopes: &[Vec<(String, String)>]) -> String {
    let (prefix, local) = qname.split_once(':').unwrap_or(("", qname));
    if prefix == "xml" || prefix == "xmlns" {
        return qname.to_string();
    }
    let uri = scopes
        .iter()
        .rev()
        .flat_map(|s| s.iter().rev())
        .find(|(p, _)| p == prefix)
        .map(|(_, u)| u.as_str());
    match uri {
        Some(u) => match NAMESPACES.iter().find(|(_, nu)| *nu == u) {
            Some((p, _)) => format!("{p}:{local}"),
            None if prefix.is_empty() => local.to_string(),
            None => qname.to_string(),
        },
        None => qname.to_string(),
    }
}

fn entity(name: &str) -> Option<char> {
    Some(match name {
        "lt" => '<',
        "gt" => '>',
        "amp" => '&',
        "quot" => '"',
        "apos" => '\'',
        _ => {
            let n = name.strip_prefix('#')?;
            let v = match n.strip_prefix(['x', 'X']) {
                Some(h) => u32::from_str_radix(h, 16).ok()?,
                None => n.parse().ok()?,
            };
            char::from_u32(v)?
        }
    })
}

/// Attribute values unescaped (predefined and numeric references; unknown ones kept).
fn unescape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        match rest
            .find(';')
            .and_then(|j| entity(&rest[1..j]).map(|c| (c, j)))
        {
            Some((c, j)) => {
                out.push(c);
                rest = &rest[j + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn parse_tree(s: &str) -> Result<Node, XmpError> {
    let mut reader = quick_xml::Reader::from_str(s);
    reader.config_mut().check_end_names = false;
    let mut stack: Vec<Node> = vec![Node {
        name: "#root".into(),
        ..Default::default()
    }];
    let mut scopes: Vec<Vec<(String, String)>> = vec![vec![(
        "xml".into(),
        "http://www.w3.org/XML/1998/namespace".into(),
    )]];
    let mut count = 0usize;
    loop {
        let ev = reader
            .read_event()
            .map_err(|e| XmpError::Xml(e.to_string()))?;
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                count += 1;
                if count > MAX_NODES || stack.len() > MAX_DEPTH {
                    return Err(XmpError::TooComplex);
                }
                let mut scope = Vec::new();
                let mut raw_attrs = Vec::new();
                for a in e.attributes().with_checks(false).flatten() {
                    let k = String::from_utf8_lossy(a.key.as_ref()).into_owned();
                    let v = unescape_attr(&String::from_utf8_lossy(&a.value));
                    if k == "xmlns" {
                        scope.push((String::new(), v));
                    } else if let Some(p) = k.strip_prefix("xmlns:") {
                        scope.push((p.to_string(), v));
                    } else {
                        raw_attrs.push((k, v));
                    }
                }
                scopes.push(scope);
                let name = canonical(&String::from_utf8_lossy(e.name().as_ref()), &scopes);
                let attrs = raw_attrs
                    .into_iter()
                    .map(|(k, v)| (canonical(&k, &scopes), v))
                    .collect();
                let node = Node {
                    name,
                    attrs,
                    ..Default::default()
                };
                if matches!(ev, Event::Empty(_)) {
                    scopes.pop();
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    }
                } else {
                    stack.push(node);
                }
            }
            Event::End(_) => {
                if stack.len() > 1 {
                    if let Some(n) = stack.pop() {
                        scopes.pop();
                        if let Some(parent) = stack.last_mut() {
                            parent.children.push(n);
                        }
                    }
                }
            }
            Event::Text(t) => {
                if let Some(top) = stack.last_mut() {
                    top.text
                        .push_str(&t.decode().map_err(|e| XmpError::Xml(e.to_string()))?);
                }
            }
            Event::CData(t) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Event::GeneralRef(r) => {
                let name = String::from_utf8_lossy(&r).into_owned();
                if let Some(top) = stack.last_mut() {
                    match entity(&name) {
                        Some(c) => top.text.push(c),
                        None => {
                            top.text.push('&');
                            top.text.push_str(&name);
                            top.text.push(';');
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    // Unterminated elements are closed, leniently, as a reader of other apps' files must.
    while stack.len() > 1 {
        if let Some(n) = stack.pop() {
            if let Some(parent) = stack.last_mut() {
                parent.children.push(n);
            }
        }
    }
    stack.pop().ok_or_else(|| XmpError::Xml("empty".into()))
}

fn is_rdf_meta_attr(k: &str) -> bool {
    matches!(
        k,
        "rdf:about"
            | "rdf:ID"
            | "rdf:nodeID"
            | "xml:lang"
            | "rdf:parseType"
            | "rdf:resource"
            | "rdf:datatype"
    ) || k.starts_with("xmlns")
}

fn collect_description(d: &Node, prefix: &str, props: &mut BTreeMap<String, Vec<String>>) {
    for (k, v) in &d.attrs {
        if !is_rdf_meta_attr(k) {
            props
                .entry(format!("{prefix}{k}"))
                .or_default()
                .push(v.clone());
        }
    }
    for c in &d.children {
        collect_property(c, prefix, props);
    }
}

fn collect_property(p: &Node, prefix: &str, props: &mut BTreeMap<String, Vec<String>>) {
    let key = format!("{prefix}{}", p.name);
    if let Some(arr) = p
        .children
        .iter()
        .find(|c| matches!(c.name.as_str(), "rdf:Seq" | "rdf:Bag" | "rdf:Alt"))
    {
        let mut items: Vec<(bool, String)> = arr
            .children
            .iter()
            .filter(|li| li.name == "rdf:li")
            .map(|li| {
                (
                    li.attr("xml:lang") == Some("x-default"),
                    li.attr("rdf:resource")
                        .map(str::to_string)
                        .unwrap_or_else(|| li.text.trim().to_string()),
                )
            })
            .collect();
        if arr.name == "rdf:Alt" {
            items.sort_by_key(|(default, _)| !*default);
        }
        props
            .entry(key)
            .or_default()
            .extend(items.into_iter().map(|(_, v)| v));
        return;
    }
    let struct_prefix = format!("{key}/");
    if p.attr("rdf:parseType") == Some("Resource") {
        for c in &p.children {
            collect_property(c, &struct_prefix, props);
        }
        return;
    }
    if let Some(d) = p.children.iter().find(|c| c.name == "rdf:Description") {
        collect_description(d, &struct_prefix, props);
        return;
    }
    let field_attrs: Vec<&(String, String)> = p
        .attrs
        .iter()
        .filter(|(k, _)| !is_rdf_meta_attr(k))
        .collect();
    if !field_attrs.is_empty() && p.children.is_empty() && p.text.trim().is_empty() {
        for (k, v) in field_attrs {
            props
                .entry(format!("{struct_prefix}{k}"))
                .or_default()
                .push(v.clone());
        }
        return;
    }
    let v = p
        .attr("rdf:resource")
        .map(str::to_string)
        .unwrap_or_else(|| p.text.trim().to_string());
    props.entry(key).or_default().push(v);
}

fn find_descriptions<'a>(n: &'a Node, out: &mut Vec<&'a Node>) {
    if n.name == "rdf:Description" {
        out.push(n);
        return;
    }
    for c in &n.children {
        find_descriptions(c, out);
    }
}

// ── The merge (after LightCraft's `xmp_merge.rs`) ────────────────────────────────────
//
// The existing packet is scanned once, namespace-resolved, with byte positions. Only
// top-level properties of the `rdf:Description`s directly under `rdf:RDF` are considered,
// in attribute form (`xmp:Rating="3"`) and element form (`<dc:subject>…</dc:subject>`).
// Owned properties are removed and written from the fresh packet; everything else is
// copied unchanged. The written properties go into one new `rdf:Description` (declaring
// its own namespaces, so the existing prefix bindings never matter) just before
// `</rdf:RDF>`; a description the removals empty is dropped, so repeated saves do not
// accumulate. The result is parsed again, and every written property must read back.

/// Which properties [`merge_packets`] takes from the fresh packet: canonical `prefix:name`,
/// or `prefix:*` for a whole namespace.
#[derive(Clone, Copy, Debug, Default)]
struct MergeRules<'a> {
    /// Always replaced: removed from the existing packet, written from the fresh one if
    /// it has them (so a cleared field is cleared in the file too).
    owned: &'a [&'a str],
    /// Replaced only when the fresh packet has them; otherwise the existing value stays.
    owned_if_present: &'a [&'a str],
}

fn listed(list: &[&str], name: &str) -> bool {
    list.iter().any(|p| match p.strip_suffix('*') {
        Some(ns) => name.starts_with(ns),
        None => *p == name,
    })
}

#[derive(Debug)]
struct Attr {
    raw: String,
    name: String,
    /// The value as written (still escaped).
    value: String,
}

#[derive(Debug)]
struct Prop {
    name: String,
    start: usize,
    end: usize,
}

#[derive(Debug, Default)]
struct Desc {
    tag_start: usize,
    tag_end: usize,
    /// The element name as written (`rdf:Description`, or another prefix for RDF's).
    raw_name: String,
    empty: bool,
    attrs: Vec<Attr>,
    props: Vec<Prop>,
    /// The end of `</rdf:Description>` (`tag_end` when self-closing).
    end: usize,
}

#[derive(Debug, Default)]
struct Scan {
    descs: Vec<Desc>,
    /// Where the first `</rdf:RDF>` starts.
    rdf_close: Option<usize>,
}

enum Kind {
    Other,
    Rdf,
    Desc(usize),
    Prop {
        desc: usize,
        start: usize,
        name: String,
    },
}

fn pos(r: &quick_xml::Reader<&[u8]>) -> usize {
    usize::try_from(r.buffer_position()).unwrap_or(usize::MAX)
}

/// The descriptions, their top-level properties and `</rdf:RDF>` in a packet.
fn scan(s: &str) -> Result<Scan, XmpError> {
    let mut reader = quick_xml::Reader::from_reader(s.as_bytes());
    reader.config_mut().check_end_names = true;
    let mut out = Scan::default();
    let mut stack: Vec<Kind> = Vec::new();
    let mut scopes: Vec<Vec<(String, String)>> = vec![vec![(
        "xml".into(),
        "http://www.w3.org/XML/1998/namespace".into(),
    )]];
    let mut count = 0usize;
    loop {
        let before = pos(&reader);
        let ev = reader
            .read_event()
            .map_err(|e| XmpError::Xml(e.to_string()))?;
        let after = pos(&reader);
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                count += 1;
                if count > MAX_NODES || stack.len() > MAX_DEPTH {
                    return Err(XmpError::TooComplex);
                }
                let empty = matches!(ev, Event::Empty(_));
                let mut scope = Vec::new();
                let mut raw_attrs = Vec::new();
                // Repeated names refused with a set, in linear time (quick-xml's own
                // check is quadratic).
                let mut seen = HashSet::new();
                for a in e.attributes().with_checks(false) {
                    let a = a.map_err(|e| XmpError::Xml(e.to_string()))?;
                    let k = String::from_utf8_lossy(a.key.as_ref()).into_owned();
                    if !seen.insert(k.clone()) {
                        return Err(XmpError::Xml(format!("repeated attribute {k}")));
                    }
                    let v = String::from_utf8_lossy(&a.value).into_owned();
                    if k == "xmlns" {
                        scope.push((String::new(), v.clone()));
                    } else if let Some(p) = k.strip_prefix("xmlns:") {
                        scope.push((p.to_string(), v.clone()));
                    }
                    raw_attrs.push((k, v));
                }
                scopes.push(scope);
                let raw_name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let name = canonical(&raw_name, &scopes);
                let kind = match stack.last() {
                    Some(Kind::Rdf) if name == "rdf:Description" => {
                        let attrs = raw_attrs
                            .into_iter()
                            .map(|(raw, value)| Attr {
                                name: canonical(&raw, &scopes),
                                raw,
                                value,
                            })
                            .collect();
                        out.descs.push(Desc {
                            tag_start: before,
                            tag_end: after,
                            raw_name,
                            empty,
                            attrs,
                            props: Vec::new(),
                            end: after,
                        });
                        Kind::Desc(out.descs.len() - 1)
                    }
                    Some(Kind::Desc(i)) => Kind::Prop {
                        desc: *i,
                        start: before,
                        name,
                    },
                    _ if name == "rdf:RDF" && out.rdf_close.is_none() => Kind::Rdf,
                    _ => Kind::Other,
                };
                if empty {
                    scopes.pop();
                    if let Kind::Prop { desc, start, name } = kind {
                        if let Some(d) = out.descs.get_mut(desc) {
                            d.props.push(Prop {
                                name,
                                start,
                                end: after,
                            });
                        }
                    }
                } else {
                    stack.push(kind);
                }
            }
            Event::End(_) => {
                scopes.pop();
                match stack.pop() {
                    Some(Kind::Prop { desc, start, name }) => {
                        if let Some(d) = out.descs.get_mut(desc) {
                            d.props.push(Prop {
                                name,
                                start,
                                end: after,
                            });
                        }
                    }
                    Some(Kind::Desc(i)) => {
                        if let Some(d) = out.descs.get_mut(i) {
                            d.end = after;
                        }
                    }
                    Some(Kind::Rdf) => out.rdf_close = out.rdf_close.or(Some(before)),
                    Some(Kind::Other) => {}
                    None => return Err(XmpError::Xml("unbalanced end tag".into())),
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(XmpError::Xml("unclosed element".into()));
    }
    Ok(out)
}

/// `start` moved back over the indentation and line break before it, so a removed
/// property takes its line with it.
fn line_start(s: &str, start: usize) -> usize {
    let b = s.as_bytes();
    let mut i = start;
    while i > 0 && matches!(b.get(i - 1), Some(b' ' | b'\t')) {
        i -= 1;
    }
    if i > 0 && b.get(i - 1) == Some(&b'\n') {
        i -= 1;
        if i > 0 && b.get(i - 1) == Some(&b'\r') {
            i -= 1;
        }
    }
    i
}

fn merge_packets(existing: &str, fresh: &str, rules: MergeRules) -> Result<String, XmpError> {
    let (bom, old) = match existing.strip_prefix('\u{feff}') {
        Some(rest) => ("\u{feff}", rest),
        None => ("", existing),
    };
    parse(old)?;
    let ex = scan(old)?;
    let rdf_close = ex
        .rdf_close
        .ok_or_else(|| XmpError::Xml("no rdf:RDF element".into()))?;
    let new = scan(fresh)?;
    let fresh_desc = new
        .descs
        .first()
        .ok_or_else(|| XmpError::Xml("the new packet has no rdf:Description".into()))?;
    let fresh_names: BTreeSet<&str> = fresh_desc
        .props
        .iter()
        .map(|p| p.name.as_str())
        .chain(fresh_desc.attrs.iter().map(|a| a.name.as_str()))
        .collect();
    let remove = |name: &str| {
        listed(rules.owned, name)
            || (listed(rules.owned_if_present, name) && fresh_names.contains(name))
    };
    let write = |name: &str| listed(rules.owned, name) || listed(rules.owned_if_present, name);

    // Edits on `old`: (start, end, replacement), applied front to back.
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for d in &ex.descs {
        let gone_attrs: Vec<&Attr> = d
            .attrs
            .iter()
            .filter(|a| !is_rdf_meta_attr(&a.name) && remove(&a.name))
            .collect();
        let gone_props: Vec<&Prop> = d.props.iter().filter(|p| remove(&p.name)).collect();
        if gone_attrs.is_empty() && gone_props.is_empty() {
            continue;
        }
        let attrs_left = d
            .attrs
            .iter()
            .any(|a| !is_rdf_meta_attr(&a.name) && !remove(&a.name));
        if !attrs_left && gone_props.len() == d.props.len() {
            // Nothing of its own left: the whole description goes.
            edits.push((line_start(old, d.tag_start), d.end, String::new()));
            continue;
        }
        if !gone_attrs.is_empty() {
            let mut tag = format!("<{}", d.raw_name);
            for a in d
                .attrs
                .iter()
                .filter(|a| is_rdf_meta_attr(&a.name) || !remove(&a.name))
            {
                tag.push_str(&format!(
                    "\n    {}=\"{}\"",
                    a.raw,
                    a.value.replace('"', "&quot;")
                ));
            }
            tag.push_str(if d.empty { "/>" } else { ">" });
            edits.push((d.tag_start, d.tag_end, tag));
        }
        for p in gone_props {
            edits.push((line_start(old, p.start), p.end, String::new()));
        }
    }
    // The new description: its own namespace declarations, then what is written. Owned
    // properties in attribute form are written as attributes, the rest as elements.
    let mut block = String::from(
        "\n  <rdf:Description rdf:about=\"\"\n    xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"",
    );
    for a in fresh_desc
        .attrs
        .iter()
        .filter(|a| a.raw.starts_with("xmlns:") && a.raw != "xmlns:rdf")
    {
        block.push_str(&format!("\n    {}=\"{}\"", a.raw, a.value));
    }
    let mut written = 0;
    for a in fresh_desc
        .attrs
        .iter()
        .filter(|a| !is_rdf_meta_attr(&a.name) && write(&a.name))
    {
        block.push_str(&format!("\n    {}=\"{}\"", a.raw, a.value));
        written += 1;
    }
    block.push('>');
    for p in fresh_desc.props.iter().filter(|p| write(&p.name)) {
        let text = fresh
            .get(p.start..p.end)
            .ok_or_else(|| XmpError::Xml("bad property span".into()))?;
        block.push_str("\n   ");
        block.push_str(text);
        written += 1;
    }
    block.push_str("\n  </rdf:Description>");
    if written > 0 {
        let at = old
            .get(..rdf_close)
            .map_or(rdf_close, |h| h.trim_end().len());
        edits.push((at, at, block));
    }
    edits.sort_by_key(|e| (e.0, e.1));
    // Overlapping edits cannot happen (properties sit inside their description, which is
    // either dropped whole or edited piecewise), but nothing is spliced blindly.
    let mut out = String::with_capacity(old.len() + 4096);
    out.push_str(bom);
    let mut at = 0;
    for (s, e, r) in &edits {
        if *s < at {
            return Err(XmpError::Xml("overlapping edits".into()));
        }
        out.push_str(
            old.get(at..*s)
                .ok_or_else(|| XmpError::Xml("bad span".into()))?,
        );
        out.push_str(r);
        at = *e;
    }
    out.push_str(
        old.get(at..)
            .ok_or_else(|| XmpError::Xml("bad span".into()))?,
    );

    // Read back: everything written must come out as written.
    let merged = parse(&out)?;
    let want = parse(fresh)?;
    for (k, v) in &want.properties {
        let top = k.split('/').next().unwrap_or(k);
        if write(top) && merged.properties.get(k) != Some(v) {
            return Err(XmpError::Xml(format!("{k} did not survive the merge")));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(rating: u8, label: Option<&str>, words: &[&str]) -> AssetMetadata {
        let mut m = AssetMetadata {
            rating,
            color_label: label.map(str::to_string),
            ..Default::default()
        };
        m.add_keywords(&words.join(","));
        m
    }

    /// A sidecar as Lightroom Classic writes one: attributes, develop settings, history.
    const LIGHTROOM: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core 7.0">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    xmp:Rating="4"
    xmp:Label="Green"
    crs:Exposure2012="+0.35">
   <dc:subject>
    <rdf:Bag>
     <rdf:li>harbour</rdf:li>
     <rdf:li>boats &amp; nets</rdf:li>
    </rdf:Bag>
   </dc:subject>
   <crs:ToneCurvePV2012>
    <rdf:Seq>
     <rdf:li>0, 0</rdf:li>
     <rdf:li>255, 255</rdf:li>
    </rdf:Seq>
   </crs:ToneCurvePV2012>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#;

    #[test]
    fn lightroom_marks_are_read() {
        let m = parse(LIGHTROOM).unwrap().marks().unwrap();
        assert_eq!(m, marks(4, Some("green"), &["harbour", "boats & nets"]));
    }

    #[test]
    fn darktable_marks_are_read_with_any_prefix() {
        let dt = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
          <rdf:Description rdf:about="" xmlns:a="http://ns.adobe.com/xap/1.0/" xmlns:d="http://darktable.sf.net/"
            xmlns:dc="http://purl.org/dc/elements/1.1/" a:Rating="-1">
           <d:colorlabels><rdf:Seq><rdf:li>3</rdf:li><rdf:li>0</rdf:li></rdf:Seq></d:colorlabels>
           <dc:subject><rdf:Bag><rdf:li>darktable|format|ARW</rdf:li></rdf:Bag></dc:subject>
          </rdf:Description></rdf:RDF></x:xmpmeta>"#;
        let m = parse(dt).unwrap().marks().unwrap();
        assert_eq!(m.flag, Flag::Reject);
        assert_eq!(m.rating, 0);
        assert_eq!(m.color_label.as_deref(), Some("blue"));
        assert_eq!(m.keywords, ["darktable|format|ARW"]);
    }

    #[test]
    fn a_packet_that_says_nothing_seeds_nothing() {
        let x = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
          <rdf:Description rdf:about="" xmlns:tiff="http://ns.adobe.com/tiff/1.0/" tiff:Make="Sony"/>
          </rdf:RDF></x:xmpmeta>"#;
        assert_eq!(parse(x).unwrap().marks(), None);
        assert!(parse("<not closed").map_or(true, |x| x.marks().is_none()));
    }

    #[test]
    fn an_export_packet_reads_back_as_written() {
        let m = marks(3, Some("purple"), &["a <b>", "c"]);
        let p = packet(&ExportXmp {
            marks: Some(&m),
            creator: Some("Carter & co"),
            rights: Some("© 2026"),
        })
        .unwrap();
        let x = parse(&p).unwrap();
        assert_eq!(x.marks().unwrap(), m);
        assert_eq!(x.first("dc:creator"), Some("Carter & co"));
        assert_eq!(x.first("dc:rights"), Some("© 2026"));
        let reject = AssetMetadata {
            flag: Flag::Reject,
            ..Default::default()
        };
        let p = packet(&ExportXmp {
            marks: Some(&reject),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(parse(&p).unwrap().first("xmp:Rating"), Some("-1"));
        assert_eq!(packet(&ExportXmp::default()), None);
    }

    #[test]
    fn a_merge_replaces_the_marks_and_keeps_the_rest_byte_for_byte() {
        let m = marks(2, None, &["lighthouse"]);
        let out = merge(LIGHTROOM, &m).unwrap();
        let x = parse(&out).unwrap();
        assert_eq!(x.marks().unwrap(), m, "{out}");
        assert_eq!(x.first("crs:Exposure2012"), Some("+0.35"));
        assert!(out.contains("   <crs:ToneCurvePV2012>\n    <rdf:Seq>\n     <rdf:li>0, 0</rdf:li>"));
        assert!(!out.contains("Green") && !out.contains("harbour"), "{out}");
        // Saving again changes nothing.
        assert_eq!(merge(&out, &m).unwrap(), out);
        // Clearing every mark clears them in the file too.
        let cleared = merge(&out, &AssetMetadata::default()).unwrap();
        assert_eq!(parse(&cleared).unwrap().marks(), None, "{cleared}");
        assert!(cleared.contains("crs:Exposure2012=\"+0.35\""));
    }

    #[test]
    fn a_malformed_sidecar_is_not_merged_into() {
        assert!(merge("<x:xmpmeta><rdf:RDF><unclosed>", &marks(1, None, &[])).is_err());
        assert!(merge("plain text", &marks(1, None, &[])).is_err());
    }

    #[test]
    fn the_sidecar_beside_a_photo_seeds_it_and_follows_its_marks() {
        let dir = std::env::temp_dir().join(format!("duckfoot-xmp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("IMG_1.ARW");
        std::fs::write(&photo, b"not really a raw").unwrap();
        assert_eq!(seed_metadata(&photo), None);
        assert_eq!(update_sidecar(&photo, &marks(1, None, &[])).unwrap(), None);
        assert!(!dir.join("IMG_1.xmp").exists(), "a sidecar was made");

        let lr = dir.join("IMG_1.xmp");
        std::fs::write(&lr, LIGHTROOM).unwrap();
        assert_eq!(seed_metadata(&photo).unwrap().rating, 4);
        let m = marks(5, Some("red"), &[]);
        assert_eq!(update_sidecar(&photo, &m).unwrap(), Some(lr.clone()));
        assert_eq!(seed_metadata(&photo).unwrap(), m);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_packet_inside_a_file_seeds_it() {
        let dir = std::env::temp_dir().join(format!("duckfoot-xmp-in-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("in.jpg");
        let m = marks(3, Some("yellow"), &["x"]);
        let p = packet(&ExportXmp {
            marks: Some(&m),
            ..Default::default()
        })
        .unwrap();
        let mut bytes = b"\xff\xd8\xff\xe1junk http://ns.adobe.com/xap/1.0/\0".to_vec();
        bytes.extend(p.as_bytes());
        bytes.extend(b"\xff\xd9");
        std::fs::write(&photo, bytes).unwrap();
        assert_eq!(seed_metadata(&photo).unwrap(), m);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
