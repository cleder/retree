use pyo3::exceptions::{PyIndexError, PySyntaxError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBytes, PyDict, PyList, PyString};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

// ---------------------------------------------------------------------------
// Global namespace registry
// ---------------------------------------------------------------------------

static NS_REGISTRY: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();

fn ns_registry() -> &'static RwLock<HashMap<String, String>> {
    NS_REGISTRY.get_or_init(|| RwLock::new(HashMap::new()))
}

// ---------------------------------------------------------------------------
// ParseError – Python exception
// ---------------------------------------------------------------------------

pyo3::create_exception!(_rust_etree, ParseError, PySyntaxError);

// ---------------------------------------------------------------------------
// Arena node
// ---------------------------------------------------------------------------

struct Node {
    tag: String,
    attrib: Vec<(String, String)>,
    text: Option<String>,
    tail: Option<String>,
    parent: Option<usize>,
    children: Vec<usize>,
}

impl Node {
    fn new(tag: String, parent: Option<usize>) -> Self {
        Node {
            tag,
            attrib: Vec::new(),
            text: None,
            tail: None,
            parent,
            children: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// PyDocument – owns the arena
// ---------------------------------------------------------------------------

#[pyclass(subclass)]
pub struct PyDocument {
    nodes: Vec<Node>,
}

impl PyDocument {
    fn add_node(&mut self, node: Node) -> usize {
        let id = self.nodes.len();
        self.nodes.push(node);
        id
    }
}

// ---------------------------------------------------------------------------
// Helper: escape text/attribute values
// ---------------------------------------------------------------------------

fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// XML parsing
// ---------------------------------------------------------------------------

fn parse_xml_bytes(py: Python<'_>, data: &[u8]) -> PyResult<Py<PyDocument>> {
    let mut doc = PyDocument { nodes: Vec::new() };
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(false);

    let mut stack: Vec<usize> = Vec::new();
    let mut last_closed: Option<usize> = None;
    let mut buf = Vec::new();

    macro_rules! parse_attrs {
        ($e:expr, $node:expr) => {{
            let decoder = reader.decoder();
            for attr in $e.attributes() {
                let attr = attr
                    .map_err(|e| ParseError::new_err(format!("attribute: {e}")))?;
                let key = std::str::from_utf8(attr.key.0)
                    .map_err(|e| ParseError::new_err(format!("attr key utf8: {e}")))?
                    .to_owned();
                let val = attr
                    .decode_and_unescape_value(decoder)
                    .map_err(|e| ParseError::new_err(format!("attr value: {e}")))?
                    .into_owned();
                $node.attrib.push((key, val));
            }
        }};
    }

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                last_closed = None;
                let tag = std::str::from_utf8(e.name().0)
                    .map_err(|e| ParseError::new_err(format!("tag utf8: {e}")))?
                    .to_owned();
                let parent = stack.last().copied();
                let mut node = Node::new(tag, parent);
                parse_attrs!(e, node);
                let id = doc.add_node(node);
                if let Some(p) = parent {
                    doc.nodes[p].children.push(id);
                }
                stack.push(id);
            }
            Ok(Event::End(_)) => {
                last_closed = stack.pop();
            }
            Ok(Event::Empty(ref e)) => {
                last_closed = None;
                let tag = std::str::from_utf8(e.name().0)
                    .map_err(|e| ParseError::new_err(format!("tag utf8: {e}")))?
                    .to_owned();
                let parent = stack.last().copied();
                let mut node = Node::new(tag, parent);
                parse_attrs!(e, node);
                let id = doc.add_node(node);
                if let Some(p) = parent {
                    doc.nodes[p].children.push(id);
                }
                last_closed = Some(id);
            }
            Ok(Event::Text(ref t)) => {
                let text = t
                    .unescape()
                    .map_err(|e| ParseError::new_err(format!("text: {e}")))?
                    .into_owned();
                if let Some(lc) = last_closed {
                    doc.nodes[lc].tail.get_or_insert_with(String::new).push_str(&text);
                } else if let Some(&cur) = stack.last() {
                    doc.nodes[cur].text.get_or_insert_with(String::new).push_str(&text);
                }
            }
            Ok(Event::CData(ref cd)) => {
                let text = std::str::from_utf8(cd.as_ref())
                    .map_err(|e| ParseError::new_err(format!("cdata utf8: {e}")))?
                    .to_owned();
                if let Some(lc) = last_closed {
                    doc.nodes[lc].tail.get_or_insert_with(String::new).push_str(&text);
                } else if let Some(&cur) = stack.last() {
                    doc.nodes[cur].text.get_or_insert_with(String::new).push_str(&text);
                }
            }
            Ok(Event::Comment(ref c)) => {
                let comment_text = c
                    .unescape()
                    .map_err(|e| ParseError::new_err(format!("comment: {e}")))?
                    .into_owned();
                last_closed = None;
                let parent = stack.last().copied();
                let mut node = Node::new("comment".to_string(), parent);
                node.text = Some(comment_text);
                let id = doc.add_node(node);
                if let Some(p) = parent {
                    doc.nodes[p].children.push(id);
                }
                last_closed = Some(id);
            }
            Ok(Event::PI(ref pi)) => {
                let content = std::str::from_utf8(pi.as_ref())
                    .map_err(|e| ParseError::new_err(format!("PI utf8: {e}")))?
                    .to_owned();
                last_closed = None;
                let parent = stack.last().copied();
                let mut node = Node::new("pi".to_string(), parent);
                node.text = Some(content);
                let id = doc.add_node(node);
                if let Some(p) = parent {
                    doc.nodes[p].children.push(id);
                }
                last_closed = Some(id);
            }
            Ok(Event::Decl(_)) | Ok(Event::DocType(_)) => {}
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(ParseError::new_err(format!("parse error: {e}")));
            }
        }
        buf.clear();
    }

    Py::new(py, doc)
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

fn serialize_node(doc: &PyDocument, node_id: usize, buf: &mut String) {
    let node = &doc.nodes[node_id];

    if node.tag == "comment" {
        buf.push_str("<!--");
        buf.push_str(node.text.as_deref().unwrap_or(""));
        buf.push_str("-->");
        if let Some(t) = &node.tail {
            buf.push_str(&escape_text(t));
        }
        return;
    }

    if node.tag == "pi" {
        buf.push_str("<?");
        buf.push_str(node.text.as_deref().unwrap_or(""));
        buf.push_str("?>");
        if let Some(t) = &node.tail {
            buf.push_str(&escape_text(t));
        }
        return;
    }

    buf.push('<');
    buf.push_str(&node.tag);
    for (k, v) in &node.attrib {
        buf.push(' ');
        buf.push_str(k);
        buf.push_str("=\"");
        buf.push_str(&escape_attr(v));
        buf.push('"');
    }

    let has_content = node.text.is_some() || !node.children.is_empty();
    if has_content {
        buf.push('>');
        if let Some(t) = &node.text {
            buf.push_str(&escape_text(t));
        }
        for &child_id in &node.children {
            serialize_node(doc, child_id, buf);
        }
        buf.push_str("</");
        buf.push_str(&node.tag);
        buf.push('>');
    } else {
        buf.push_str(" />");
    }

    if let Some(t) = &node.tail {
        buf.push_str(&escape_text(t));
    }
}

fn collect_text(doc: &PyDocument, node_id: usize, out: &mut String) {
    let node = &doc.nodes[node_id];
    if let Some(t) = &node.text {
        out.push_str(t);
    }
    for &child_id in &node.children {
        collect_text(doc, child_id, out);
        if let Some(t) = &doc.nodes[child_id].tail {
            out.push_str(t);
        }
    }
}

// HTML void elements (self-closing)
const HTML_VOID: &[&str] = &[
    "area", "base", "basefont", "br", "col", "embed", "frame", "hr", "img",
    "input", "isindex", "link", "meta", "param", "source", "track", "wbr",
];

fn serialize_html(doc: &PyDocument, node_id: usize, buf: &mut String) {
    let node = &doc.nodes[node_id];
    let tag_lower = node.tag.to_lowercase();
    buf.push('<');
    buf.push_str(&node.tag);
    for (k, v) in &node.attrib {
        buf.push(' ');
        buf.push_str(k);
        if !v.is_empty() {
            buf.push_str("=\"");
            buf.push_str(v);
            buf.push('"');
        }
    }
    buf.push('>');
    if !HTML_VOID.contains(&tag_lower.as_str()) {
        if let Some(t) = &node.text {
            buf.push_str(t);
        }
        for &child_id in &node.children {
            serialize_html(doc, child_id, buf);
            if let Some(t) = &doc.nodes[child_id].tail {
                buf.push_str(t);
            }
        }
        buf.push_str("</");
        buf.push_str(&node.tag);
        buf.push('>');
    }
    if let Some(t) = &node.tail {
        buf.push_str(t);
    }
}

// ---------------------------------------------------------------------------
// XPath-like find helpers
// ---------------------------------------------------------------------------

fn find_element(doc: &PyDocument, root_id: usize, path: &str) -> PyResult<Option<usize>> {
    let results = find_all_elements(doc, root_id, path)?;
    Ok(results.into_iter().next())
}

fn find_all_elements(doc: &PyDocument, root_id: usize, path: &str) -> PyResult<Vec<usize>> {
    let path = path.trim();

    if let Some(rest) = path.strip_prefix(".//") {
        let tag = rest;
        let mut results = Vec::new();
        find_descendants(doc, root_id, tag, false, &mut results);
        return Ok(results);
    }

    if path == "." {
        return Ok(vec![root_id]);
    }

    if !path.contains('/') {
        let node = doc
            .nodes
            .get(root_id)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))?;
        let results = node
            .children
            .iter()
            .filter(|&&id| {
                let child = &doc.nodes[id];
                path == "*" || child.tag == path
            })
            .copied()
            .collect();
        return Ok(results);
    }

    let parts: Vec<&str> = path.split('/').collect();
    let mut current_ids = vec![root_id];
    for part in &parts {
        let mut next_ids = Vec::new();
        for &cid in &current_ids {
            let node = &doc.nodes[cid];
            for &child_id in &node.children {
                let child = &doc.nodes[child_id];
                if *part == "*" || child.tag == *part {
                    next_ids.push(child_id);
                }
            }
        }
        current_ids = next_ids;
    }
    Ok(current_ids)
}

fn find_descendants(doc: &PyDocument, node_id: usize, tag: &str, include_self: bool, out: &mut Vec<usize>) {
    if include_self {
        let node = &doc.nodes[node_id];
        if tag == "*" || node.tag == tag {
            out.push(node_id);
        }
    }
    let children: Vec<usize> = doc.nodes[node_id].children.clone();
    for child_id in children {
        find_descendants(doc, child_id, tag, true, out);
    }
}

fn collect_iter(doc: &PyDocument, node_id: usize, tag: Option<&str>, out: &mut Vec<usize>) {
    let node = &doc.nodes[node_id];
    let matches = match tag {
        None | Some("*") => true,
        Some(t) => node.tag == t,
    };
    if matches {
        out.push(node_id);
    }
    for &child_id in &node.children {
        collect_iter(doc, child_id, tag, out);
    }
}

// ---------------------------------------------------------------------------
// Deep copy helper
// ---------------------------------------------------------------------------

fn deep_copy_node(src: &PyDocument, src_id: usize, parent: Option<usize>, dst: &mut PyDocument) -> usize {
    let src_node = &src.nodes[src_id];
    let new_id = dst.nodes.len();
    dst.nodes.push(Node {
        tag: src_node.tag.clone(),
        attrib: src_node.attrib.clone(),
        text: src_node.text.clone(),
        tail: src_node.tail.clone(),
        parent,
        children: Vec::new(),
    });
    let child_ids: Vec<usize> = src.nodes[src_id].children.clone();
    for child_src_id in child_ids {
        let child_new_id = deep_copy_node(src, child_src_id, Some(new_id), dst);
        dst.nodes[new_id].children.push(child_new_id);
    }
    new_id
}

// ---------------------------------------------------------------------------
// Iterator types
// ---------------------------------------------------------------------------

#[pyclass]
pub struct PyElementIter {
    children: Vec<usize>,
    pos: usize,
    doc: Py<PyDocument>,
}

#[pymethods]
impl PyElementIter {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyElement>> {
        if self.pos >= self.children.len() {
            return Ok(None);
        }
        let id = self.children[self.pos];
        self.pos += 1;
        Ok(Some(PyElement {
            node_id: id,
            doc: self.doc.clone_ref(py),
        }))
    }
}

#[pyclass]
pub struct PyTreeIter {
    ids: Vec<usize>,
    pos: usize,
    doc: Py<PyDocument>,
}

#[pymethods]
impl PyTreeIter {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyElement>> {
        if self.pos >= self.ids.len() {
            return Ok(None);
        }
        let id = self.ids[self.pos];
        self.pos += 1;
        Ok(Some(PyElement {
            node_id: id,
            doc: self.doc.clone_ref(py),
        }))
    }
}

// ---------------------------------------------------------------------------
// PyElement – Python-facing proxy
// ---------------------------------------------------------------------------

#[pyclass(subclass)]
pub struct PyElement {
    pub node_id: usize,
    pub doc: Py<PyDocument>,
}

impl PyElement {
    fn make_ref(&self, py: Python<'_>, node_id: usize) -> PyElement {
        PyElement {
            node_id,
            doc: self.doc.clone_ref(py),
        }
    }
}

#[pymethods]
impl PyElement {
    // --- tag property ---
    #[getter]
    fn tag(&self, py: Python<'_>) -> PyResult<String> {
        let doc = self.doc.borrow(py);
        doc.nodes
            .get(self.node_id)
            .map(|n| n.tag.clone())
            .ok_or_else(|| PyIndexError::new_err("invalid node"))
    }

    #[setter]
    fn set_tag(&self, py: Python<'_>, value: String) -> PyResult<()> {
        let mut doc = self.doc.borrow_mut(py);
        doc.nodes
            .get_mut(self.node_id)
            .map(|n| n.tag = value)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))
    }

    // --- text property ---
    #[getter]
    fn text(&self, py: Python<'_>) -> PyResult<Option<String>> {
        let doc = self.doc.borrow(py);
        doc.nodes
            .get(self.node_id)
            .map(|n| n.text.clone())
            .ok_or_else(|| PyIndexError::new_err("invalid node"))
    }

    #[setter]
    fn set_text(&self, py: Python<'_>, value: Option<String>) -> PyResult<()> {
        let mut doc = self.doc.borrow_mut(py);
        doc.nodes
            .get_mut(self.node_id)
            .map(|n| n.text = value)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))
    }

    // --- tail property ---
    #[getter]
    fn tail(&self, py: Python<'_>) -> PyResult<Option<String>> {
        let doc = self.doc.borrow(py);
        doc.nodes
            .get(self.node_id)
            .map(|n| n.tail.clone())
            .ok_or_else(|| PyIndexError::new_err("invalid node"))
    }

    #[setter]
    fn set_tail(&self, py: Python<'_>, value: Option<String>) -> PyResult<()> {
        let mut doc = self.doc.borrow_mut(py);
        doc.nodes
            .get_mut(self.node_id)
            .map(|n| n.tail = value)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))
    }

    // --- attrib property ---
    #[getter]
    fn attrib(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let doc = self.doc.borrow(py);
        let node = doc
            .nodes
            .get(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))?;
        let d = PyDict::new(py);
        for (k, v) in &node.attrib {
            d.set_item(k, v)?;
        }
        Ok(d.unbind())
    }

    #[setter]
    fn set_attrib(&self, py: Python<'_>, value: &Bound<'_, PyDict>) -> PyResult<()> {
        let mut doc = self.doc.borrow_mut(py);
        let node = doc
            .nodes
            .get_mut(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))?;
        node.attrib.clear();
        for (k, v) in value.iter() {
            let key: String = k.extract()?;
            let val: String = v.extract()?;
            node.attrib.push((key, val));
        }
        Ok(())
    }

    // --- get / set / keys / items / values ---
    fn get(&self, py: Python<'_>, key: &str, default: Option<&str>) -> PyResult<Option<String>> {
        let doc = self.doc.borrow(py);
        let node = doc
            .nodes
            .get(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))?;
        Ok(node
            .attrib
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .or_else(|| default.map(|s| s.to_owned())))
    }

    fn set(&self, py: Python<'_>, key: String, value: String) -> PyResult<()> {
        let mut doc = self.doc.borrow_mut(py);
        let node = doc
            .nodes
            .get_mut(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))?;
        if let Some(pair) = node.attrib.iter_mut().find(|(k, _)| k == &key) {
            pair.1 = value;
        } else {
            node.attrib.push((key, value));
        }
        Ok(())
    }

    fn keys(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        let doc = self.doc.borrow(py);
        let node = doc
            .nodes
            .get(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))?;
        Ok(node.attrib.iter().map(|(k, _)| k.clone()).collect())
    }

    fn values(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        let doc = self.doc.borrow(py);
        let node = doc
            .nodes
            .get(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))?;
        Ok(node.attrib.iter().map(|(_, v)| v.clone()).collect())
    }

    fn items(&self, py: Python<'_>) -> PyResult<Vec<(String, String)>> {
        let doc = self.doc.borrow(py);
        let node = doc
            .nodes
            .get(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))?;
        Ok(node.attrib.clone())
    }

    // --- __len__ ---
    fn __len__(&self, py: Python<'_>) -> PyResult<usize> {
        let doc = self.doc.borrow(py);
        doc.nodes
            .get(self.node_id)
            .map(|n| n.children.len())
            .ok_or_else(|| PyIndexError::new_err("invalid node"))
    }

    // --- __getitem__ ---
    fn __getitem__(&self, py: Python<'_>, index: isize) -> PyResult<PyElement> {
        let doc = self.doc.borrow(py);
        let node = doc
            .nodes
            .get(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))?;
        let len = node.children.len() as isize;
        let i = if index < 0 { len + index } else { index };
        if i < 0 || i >= len {
            return Err(PyIndexError::new_err("child index out of range"));
        }
        let child_id = node.children[i as usize];
        drop(doc);
        Ok(self.make_ref(py, child_id))
    }

    // --- __setitem__ ---
    fn __setitem__(&self, py: Python<'_>, index: isize, child: &PyElement) -> PyResult<()> {
        if !self.doc.is(&child.doc) {
            return Err(PyValueError::new_err("element from a different document"));
        }
        let child_id = child.node_id;
        let parent_id = self.node_id;
        let mut doc = self.doc.borrow_mut(py);
        let len = doc.nodes[parent_id].children.len() as isize;
        let i = if index < 0 { len + index } else { index };
        if i < 0 || i >= len {
            return Err(PyIndexError::new_err("child index out of range"));
        }
        let old_id = doc.nodes[parent_id].children[i as usize];
        doc.nodes[old_id].parent = None;
        doc.nodes[child_id].parent = Some(parent_id);
        doc.nodes[parent_id].children[i as usize] = child_id;
        Ok(())
    }

    // --- __delitem__ ---
    fn __delitem__(&self, py: Python<'_>, index: isize) -> PyResult<()> {
        let mut doc = self.doc.borrow_mut(py);
        let node = doc
            .nodes
            .get_mut(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))?;
        let len = node.children.len() as isize;
        let i = if index < 0 { len + index } else { index };
        if i < 0 || i >= len {
            return Err(PyIndexError::new_err("child index out of range"));
        }
        let removed_id = node.children.remove(i as usize);
        doc.nodes[removed_id].parent = None;
        Ok(())
    }

    // --- __iter__ ---
    fn __iter__(&self, py: Python<'_>) -> PyResult<PyElementIter> {
        let doc = self.doc.borrow(py);
        let node = doc
            .nodes
            .get(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))?;
        let children = node.children.clone();
        drop(doc);
        Ok(PyElementIter {
            children,
            pos: 0,
            doc: self.doc.clone_ref(py),
        })
    }

    // --- __repr__ ---
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let tag = self.tag(py)?;
        Ok(format!("<Element '{}' at 0x{:x}>", tag, self.node_id))
    }

    // --- append ---
    fn append(&self, py: Python<'_>, child: &PyElement) -> PyResult<()> {
        if !self.doc.is(&child.doc) {
            return Err(PyTypeError::new_err("element from a different document"));
        }
        let child_id = child.node_id;
        let parent_id = self.node_id;
        let mut doc = self.doc.borrow_mut(py);
        if let Some(old_parent) = doc.nodes[child_id].parent {
            if old_parent != parent_id {
                doc.nodes[old_parent].children.retain(|&c| c != child_id);
            }
        }
        doc.nodes[child_id].parent = Some(parent_id);
        if !doc.nodes[parent_id].children.contains(&child_id) {
            doc.nodes[parent_id].children.push(child_id);
        }
        Ok(())
    }

    // --- extend ---
    fn extend(&self, py: Python<'_>, children: &Bound<'_, PyAny>) -> PyResult<()> {
        let list: Vec<PyRef<'_, PyElement>> = children.extract()?;
        for child in list {
            self.append(py, &child)?;
        }
        Ok(())
    }

    // --- insert ---
    fn insert(&self, py: Python<'_>, index: isize, child: &PyElement) -> PyResult<()> {
        if !self.doc.is(&child.doc) {
            return Err(PyTypeError::new_err("element from a different document"));
        }
        let child_id = child.node_id;
        let parent_id = self.node_id;
        let mut doc = self.doc.borrow_mut(py);
        if let Some(old_parent) = doc.nodes[child_id].parent {
            if old_parent != parent_id {
                doc.nodes[old_parent].children.retain(|&c| c != child_id);
            }
        }
        doc.nodes[child_id].parent = Some(parent_id);
        let len = doc.nodes[parent_id].children.len() as isize;
        let i = if index < 0 { len + index } else { index };
        let i = i.max(0).min(len) as usize;
        if !doc.nodes[parent_id].children.contains(&child_id) {
            doc.nodes[parent_id].children.insert(i, child_id);
        }
        Ok(())
    }

    // --- remove ---
    fn remove(&self, py: Python<'_>, child: &PyElement) -> PyResult<()> {
        if !self.doc.is(&child.doc) {
            return Err(PyValueError::new_err("element not in children"));
        }
        let child_id = child.node_id;
        let parent_id = self.node_id;
        let mut doc = self.doc.borrow_mut(py);
        let pos = doc.nodes[parent_id].children.iter().position(|&c| c == child_id);
        match pos {
            Some(i) => {
                doc.nodes[parent_id].children.remove(i);
                doc.nodes[child_id].parent = None;
                Ok(())
            }
            None => Err(PyValueError::new_err("element is not in this element")),
        }
    }

    // --- clear ---
    fn clear(&self, py: Python<'_>) -> PyResult<()> {
        let mut doc = self.doc.borrow_mut(py);
        let node = doc
            .nodes
            .get_mut(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))?;
        node.text = None;
        node.tail = None;
        node.attrib.clear();
        let children: Vec<usize> = node.children.drain(..).collect();
        for c in children {
            doc.nodes[c].parent = None;
        }
        Ok(())
    }

    // --- makeelement ---
    fn makeelement(
        &self,
        py: Python<'_>,
        tag: String,
        attrib: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyElement> {
        let mut node = Node::new(tag, None);
        if let Some(d) = attrib {
            for (k, v) in d.iter() {
                let key: String = k.extract()?;
                let val: String = v.extract()?;
                node.attrib.push((key, val));
            }
        }
        let mut doc = self.doc.borrow_mut(py);
        let id = doc.add_node(node);
        drop(doc);
        Ok(self.make_ref(py, id))
    }

    // --- find ---
    fn find(
        &self,
        py: Python<'_>,
        path: &str,
        _namespaces: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Option<PyElement>> {
        let doc = self.doc.borrow(py);
        let id = find_element(&doc, self.node_id, path)?;
        drop(doc);
        Ok(id.map(|i| self.make_ref(py, i)))
    }

    // --- findall ---
    fn findall(
        &self,
        py: Python<'_>,
        path: &str,
        _namespaces: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Vec<PyElement>> {
        let doc = self.doc.borrow(py);
        let ids = find_all_elements(&doc, self.node_id, path)?;
        drop(doc);
        Ok(ids.into_iter().map(|i| self.make_ref(py, i)).collect())
    }

    // --- findtext ---
    fn findtext(
        &self,
        py: Python<'_>,
        path: &str,
        default: Option<String>,
        _namespaces: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Option<String>> {
        let doc = self.doc.borrow(py);
        if let Some(id) = find_element(&doc, self.node_id, path)? {
            Ok(Some(doc.nodes[id].text.clone().unwrap_or_default()))
        } else {
            Ok(default)
        }
    }

    // --- iter ---
    fn iter(&self, py: Python<'_>, tag: Option<String>) -> PyResult<PyTreeIter> {
        let doc = self.doc.borrow(py);
        let mut ids = Vec::new();
        collect_iter(&doc, self.node_id, tag.as_deref(), &mut ids);
        drop(doc);
        Ok(PyTreeIter {
            ids,
            pos: 0,
            doc: self.doc.clone_ref(py),
        })
    }

    // --- getparent (lxml extension) ---
    fn getparent(&self, py: Python<'_>) -> PyResult<Option<PyElement>> {
        let doc = self.doc.borrow(py);
        let node = doc
            .nodes
            .get(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))?;
        let pid = node.parent;
        drop(doc);
        Ok(pid.map(|p| self.make_ref(py, p)))
    }

    // --- getnext (lxml extension) ---
    fn getnext(&self, py: Python<'_>) -> PyResult<Option<PyElement>> {
        let doc = self.doc.borrow(py);
        let node = doc
            .nodes
            .get(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))?;
        if let Some(pid) = node.parent {
            let parent = &doc.nodes[pid];
            if let Some(pos) = parent.children.iter().position(|&c| c == self.node_id) {
                if let Some(&next_id) = parent.children.get(pos + 1) {
                    drop(doc);
                    return Ok(Some(self.make_ref(py, next_id)));
                }
            }
        }
        Ok(None)
    }

    // --- getprevious (lxml extension) ---
    fn getprevious(&self, py: Python<'_>) -> PyResult<Option<PyElement>> {
        let doc = self.doc.borrow(py);
        let node = doc
            .nodes
            .get(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))?;
        if let Some(pid) = node.parent {
            let parent = &doc.nodes[pid];
            if let Some(pos) = parent.children.iter().position(|&c| c == self.node_id) {
                if pos > 0 {
                    let prev_id = parent.children[pos - 1];
                    drop(doc);
                    return Ok(Some(self.make_ref(py, prev_id)));
                }
            }
        }
        Ok(None)
    }

    // --- getchildren (deprecated) ---
    fn getchildren(&self, py: Python<'_>) -> PyResult<Vec<PyElement>> {
        let doc = self.doc.borrow(py);
        let node = doc
            .nodes
            .get(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("invalid node"))?;
        let children: Vec<usize> = node.children.clone();
        drop(doc);
        Ok(children.into_iter().map(|id| self.make_ref(py, id)).collect())
    }

    // --- copy support ---
    fn __copy__(&self, py: Python<'_>) -> PyResult<PyElement> {
        Ok(PyElement {
            node_id: self.node_id,
            doc: self.doc.clone_ref(py),
        })
    }

    fn __deepcopy__(&self, py: Python<'_>, _memo: &Bound<'_, PyAny>) -> PyResult<PyElement> {
        let new_doc = Py::new(py, PyDocument { nodes: Vec::new() })?;
        {
            let src = self.doc.borrow(py);
            let mut dst = new_doc.borrow_mut(py);
            deep_copy_node(&src, self.node_id, None, &mut dst);
        }
        Ok(PyElement {
            node_id: 0,
            doc: new_doc,
        })
    }
}

// ---------------------------------------------------------------------------
// PyElementTree
// ---------------------------------------------------------------------------

#[pyclass(subclass)]
pub struct PyElementTree {
    root: Option<Py<PyElement>>,
}

#[pymethods]
impl PyElementTree {
    #[new]
    #[pyo3(signature = (element=None))]
    fn new(py: Python<'_>, element: Option<&PyElement>) -> PyResult<Self> {
        match element {
            None => Ok(PyElementTree { root: None }),
            Some(e) => Ok(PyElementTree {
                root: Some(Py::new(py, PyElement { node_id: e.node_id, doc: e.doc.clone_ref(py) })?),
            }),
        }
    }

    fn getroot(&self, py: Python<'_>) -> Option<PyElement> {
        self.root.as_ref().map(|r| {
            let borrowed = r.borrow(py);
            PyElement { node_id: borrowed.node_id, doc: borrowed.doc.clone_ref(py) }
        })
    }

    fn _setroot(&mut self, py: Python<'_>, element: &PyElement) -> PyResult<()> {
        self.root = Some(Py::new(py, PyElement {
            node_id: element.node_id,
            doc: element.doc.clone_ref(py),
        })?);
        Ok(())
    }

    fn find(
        &self,
        py: Python<'_>,
        path: &str,
        namespaces: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Option<PyElement>> {
        if let Some(ref root) = self.root {
            root.borrow(py).find(py, path, namespaces)
        } else {
            Ok(None)
        }
    }

    fn findall(
        &self,
        py: Python<'_>,
        path: &str,
        namespaces: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Vec<PyElement>> {
        if let Some(ref root) = self.root {
            root.borrow(py).findall(py, path, namespaces)
        } else {
            Ok(vec![])
        }
    }

    fn findtext(
        &self,
        py: Python<'_>,
        path: &str,
        default: Option<String>,
        namespaces: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Option<String>> {
        if let Some(ref root) = self.root {
            root.borrow(py).findtext(py, path, default, namespaces)
        } else {
            Ok(default)
        }
    }

    fn iter(&self, py: Python<'_>, tag: Option<String>) -> PyResult<PyTreeIter> {
        if let Some(ref root) = self.root {
            root.borrow(py).iter(py, tag)
        } else {
            Ok(PyTreeIter {
                ids: vec![],
                pos: 0,
                doc: Py::new(py, PyDocument { nodes: vec![] })?,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// PyQName
// ---------------------------------------------------------------------------

#[pyclass(subclass)]
pub struct PyQName {
    pub text: String,
}

#[pymethods]
impl PyQName {
    #[new]
    #[pyo3(signature = (text_or_uri, tag=None))]
    fn new(text_or_uri: String, tag: Option<String>) -> Self {
        let text = if let Some(t) = tag {
            format!("{{{}}}{}", text_or_uri, t)
        } else {
            text_or_uri
        };
        PyQName { text }
    }

    fn __str__(&self) -> &str {
        &self.text
    }

    fn __repr__(&self) -> String {
        format!("QName('{}')", self.text)
    }

    fn __eq__(&self, other: &PyQName) -> bool {
        self.text == other.text
    }

    fn __hash__(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        self.text.hash(&mut h);
        h.finish()
    }

    #[getter]
    fn text(&self) -> &str {
        &self.text
    }

    #[getter]
    fn namespace(&self) -> Option<String> {
        if self.text.starts_with('{') {
            let end = self.text.find('}')?;
            Some(self.text[1..end].to_owned())
        } else {
            None
        }
    }

    #[getter]
    fn localname(&self) -> String {
        if self.text.starts_with('{') {
            if let Some(end) = self.text.find('}') {
                return self.text[end + 1..].to_owned();
            }
        }
        self.text.clone()
    }
}

// ---------------------------------------------------------------------------
// Module-level functions
// ---------------------------------------------------------------------------

fn extract_bytes(py: Python<'_>, obj: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    if let Ok(s) = obj.extract::<String>() {
        return Ok(s.into_bytes());
    }
    if let Ok(b) = obj.extract::<Vec<u8>>() {
        return Ok(b);
    }
    Err(PyTypeError::new_err("expected str or bytes"))
}

#[pyfunction]
#[pyo3(signature = (text, parser=None))]
fn fromstring(py: Python<'_>, text: &Bound<'_, PyAny>, parser: Option<&Bound<'_, PyAny>>) -> PyResult<PyElement> {
    let _ = parser;
    let bytes = extract_bytes(py, text)?;
    let doc = parse_xml_bytes(py, &bytes)?;
    Ok(PyElement { node_id: 0, doc })
}

#[pyfunction]
#[pyo3(signature = (element, encoding="unicode", method="xml", xml_declaration=None, default_namespace=None, short_empty_elements=true))]
fn tostring(
    py: Python<'_>,
    element: &PyElement,
    encoding: &str,
    method: &str,
    xml_declaration: Option<bool>,
    default_namespace: Option<&str>,
    short_empty_elements: bool,
) -> PyResult<Py<PyAny>> {
    let _ = default_namespace;
    let _ = short_empty_elements;
    let doc = element.doc.borrow(py);
    let mut buf = String::new();

    if method == "text" {
        collect_text(&doc, element.node_id, &mut buf);
        if encoding == "unicode" {
            return Ok(PyString::new(py, &buf).into_any().unbind());
        }
        return Ok(PyBytes::new(py, buf.as_bytes()).into_any().unbind());
    }

    let add_decl = matches!(xml_declaration, Some(true))
        || (xml_declaration.is_none() && encoding != "unicode");

    if add_decl {
        let enc_name = if encoding == "unicode" { "utf-8" } else { encoding };
        buf.push_str(&format!("<?xml version='1.0' encoding='{enc_name}'?>\n"));
    }

    if method == "html" {
        serialize_html(&doc, element.node_id, &mut buf);
    } else {
        serialize_node(&doc, element.node_id, &mut buf);
    }

    if encoding == "unicode" {
        Ok(PyString::new(py, &buf).into_any().unbind())
    } else {
        Ok(PyBytes::new(py, buf.as_bytes()).into_any().unbind())
    }
}

#[pyfunction]
#[pyo3(name = "XML")]
#[pyo3(signature = (text, parser=None))]
fn xml_func(py: Python<'_>, text: &Bound<'_, PyAny>, parser: Option<&Bound<'_, PyAny>>) -> PyResult<PyElement> {
    fromstring(py, text, parser)
}

#[pyfunction]
#[pyo3(name = "XMLID")]
#[pyo3(signature = (text, parser=None))]
fn xmlid(py: Python<'_>, text: &Bound<'_, PyAny>, parser: Option<&Bound<'_, PyAny>>) -> PyResult<(PyElement, Py<PyDict>)> {
    let root = fromstring(py, text, parser)?;
    let id_dict = PyDict::new(py);
    {
        let doc = root.doc.borrow(py);
        collect_ids(&doc, root.node_id, &id_dict, py, &root.doc)?;
    }
    Ok((root, id_dict.unbind()))
}

fn collect_ids(
    doc: &PyDocument,
    node_id: usize,
    id_dict: &Bound<'_, PyDict>,
    py: Python<'_>,
    doc_ref: &Py<PyDocument>,
) -> PyResult<()> {
    let node = &doc.nodes[node_id];
    if let Some(id_val) = node.attrib.iter().find(|(k, _)| k == "id").map(|(_, v)| v.clone()) {
        let elem = Py::new(py, PyElement {
            node_id,
            doc: doc_ref.clone_ref(py),
        })?;
        id_dict.set_item(id_val, elem)?;
    }
    let children: Vec<usize> = node.children.clone();
    for child_id in children {
        collect_ids(doc, child_id, id_dict, py, doc_ref)?;
    }
    Ok(())
}

#[pyfunction]
#[pyo3(name = "Element")]
#[pyo3(signature = (tag, attrib=None, **extra))]
fn element_func(
    py: Python<'_>,
    tag: &str,
    attrib: Option<&Bound<'_, PyDict>>,
    extra: Option<&Bound<'_, PyDict>>,
) -> PyResult<PyElement> {
    let doc = Py::new(py, PyDocument { nodes: Vec::new() })?;
    let mut node = Node::new(tag.to_owned(), None);
    if let Some(d) = attrib {
        for (k, v) in d.iter() {
            node.attrib.push((k.extract()?, v.extract()?));
        }
    }
    if let Some(d) = extra {
        for (k, v) in d.iter() {
            node.attrib.push((k.extract()?, v.extract()?));
        }
    }
    {
        let mut d = doc.borrow_mut(py);
        d.nodes.push(node);
    }
    Ok(PyElement { node_id: 0, doc })
}

#[pyfunction]
fn register_namespace(prefix: String, uri: String) -> PyResult<()> {
    let mut registry = ns_registry().write().map_err(|_| PyValueError::new_err("lock poisoned"))?;
    registry.insert(uri, prefix);
    Ok(())
}

#[pyfunction]
fn iselement(py: Python<'_>, obj: &Bound<'_, PyAny>) -> bool {
    obj.extract::<PyRef<'_, PyElement>>().is_ok()
}

// ---------------------------------------------------------------------------
// Module definition
// ---------------------------------------------------------------------------

#[pymodule]
fn _rust_etree(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyDocument>()?;
    m.add_class::<PyElement>()?;
    m.add_class::<PyElementTree>()?;
    m.add_class::<PyQName>()?;
    m.add_class::<PyElementIter>()?;
    m.add_class::<PyTreeIter>()?;
    m.add("ParseError", py.get_type::<ParseError>())?;
    m.add_function(wrap_pyfunction!(fromstring, m)?)?;
    m.add_function(wrap_pyfunction!(tostring, m)?)?;
    m.add_function(wrap_pyfunction!(xml_func, m)?)?;
    m.add_function(wrap_pyfunction!(xmlid, m)?)?;
    m.add_function(wrap_pyfunction!(element_func, m)?)?;
    m.add_function(wrap_pyfunction!(register_namespace, m)?)?;
    m.add_function(wrap_pyfunction!(iselement, m)?)?;
    Ok(())
}
