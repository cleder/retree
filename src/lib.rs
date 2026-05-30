use pyo3::exceptions::{PyIndexError, PyValueError};
use pyo3::prelude::*;

#[pyclass(subclass)]
pub struct PyNode {
    tag: String,
    text: Option<String>,
    parent: Option<usize>,
    children: Vec<usize>,
}

#[pyclass(subclass)]
pub struct PyDocument {
    nodes: Vec<PyNode>,
    root_id: usize,
}

#[pyclass(subclass)]
pub struct PyElement {
    pub node_id: usize,
    pub doc_ref: Py<PyDocument>,
}

fn parse_single_tag(input: &str) -> Result<String, &'static str> {
    let trimmed = input.trim();
    if !trimmed.starts_with('<') || !trimmed.ends_with('>') {
        return Err("invalid XML input");
    }

    let close_idx = trimmed.find('>').ok_or("invalid XML input")?;
    let start_tag = &trimmed[1..close_idx];
    if start_tag.is_empty() || start_tag.starts_with('/') {
        return Err("invalid XML input");
    }

    let expected_end = format!("</{}>", start_tag);
    if !trimmed.ends_with(&expected_end) {
        return Err("invalid XML input");
    }

    Ok(start_tag.to_string())
}

#[pymethods]
impl PyDocument {
    #[new]
    fn new(root_tag: Option<String>) -> Self {
        let tag = root_tag.unwrap_or_else(|| "root".to_string());
        let root = PyNode {
            tag,
            text: None,
            parent: None,
            children: Vec::new(),
        };

        Self {
            nodes: vec![root],
            root_id: 0,
        }
    }

    fn root(&self, py: Python<'_>, doc_ref: Py<PyDocument>) -> PyElement {
        PyElement {
            node_id: self.root_id,
            doc_ref: doc_ref.clone_ref(py),
        }
    }
}

#[pymethods]
impl PyElement {
    fn getparent(&self, py: Python<'_>) -> PyResult<Option<PyElement>> {
        let doc = self.doc_ref.borrow(py);
        let parent_id = doc
            .nodes
            .get(self.node_id)
            .and_then(|node| node.parent);

        Ok(parent_id.map(|node_id| PyElement {
            node_id,
            doc_ref: self.doc_ref.clone_ref(py),
        }))
    }

    fn getnext(&self, py: Python<'_>) -> PyResult<Option<PyElement>> {
        let doc = self.doc_ref.borrow(py);
        let Some(current) = doc.nodes.get(self.node_id) else {
            return Ok(None);
        };
        let Some(parent_id) = current.parent else {
            return Ok(None);
        };

        let Some(parent) = doc.nodes.get(parent_id) else {
            return Ok(None);
        };

        if let Some(pos) = parent.children.iter().position(|id| *id == self.node_id) {
            if let Some(next_id) = parent.children.get(pos + 1) {
                return Ok(Some(PyElement {
                    node_id: *next_id,
                    doc_ref: self.doc_ref.clone_ref(py),
                }));
            }
        }

        Ok(None)
    }

    fn getprevious(&self, py: Python<'_>) -> PyResult<Option<PyElement>> {
        let doc = self.doc_ref.borrow(py);
        let Some(current) = doc.nodes.get(self.node_id) else {
            return Ok(None);
        };
        let Some(parent_id) = current.parent else {
            return Ok(None);
        };

        let Some(parent) = doc.nodes.get(parent_id) else {
            return Ok(None);
        };

        if let Some(pos) = parent.children.iter().position(|id| *id == self.node_id) {
            if pos > 0 {
                if let Some(prev_id) = parent.children.get(pos - 1) {
                    return Ok(Some(PyElement {
                        node_id: *prev_id,
                        doc_ref: self.doc_ref.clone_ref(py),
                    }));
                }
            }
        }

        Ok(None)
    }

    fn tag(&self, py: Python<'_>) -> PyResult<String> {
        let doc = self.doc_ref.borrow(py);
        let node = doc
            .nodes
            .get(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("node out of bounds"))?;
        Ok(node.tag.clone())
    }

    fn __len__(&self, py: Python<'_>) -> PyResult<usize> {
        let doc = self.doc_ref.borrow(py);
        let node = doc
            .nodes
            .get(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("node out of bounds"))?;
        Ok(node.children.len())
    }

    fn __getitem__(&self, py: Python<'_>, index: isize) -> PyResult<PyElement> {
        let doc = self.doc_ref.borrow(py);
        let node = doc
            .nodes
            .get(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("node out of bounds"))?;

        let len = node.children.len() as isize;
        let normalized = if index < 0 { len + index } else { index };
        if !(0..len).contains(&normalized) {
            return Err(PyIndexError::new_err("child index out of range"));
        }

        let child_id = node.children[normalized as usize];
        Ok(PyElement {
            node_id: child_id,
            doc_ref: self.doc_ref.clone_ref(py),
        })
    }

    fn append(&self, py: Python<'_>, tag: String) -> PyResult<PyElement> {
        let mut doc = self.doc_ref.borrow_mut(py);

        let new_id = doc.nodes.len();
        doc.nodes.push(PyNode {
            tag,
            text: None,
            parent: Some(self.node_id),
            children: Vec::new(),
        });

        let parent = doc
            .nodes
            .get_mut(self.node_id)
            .ok_or_else(|| PyIndexError::new_err("node out of bounds"))?;
        parent.children.push(new_id);

        Ok(PyElement {
            node_id: new_id,
            doc_ref: self.doc_ref.clone_ref(py),
        })
    }
}

#[pyfunction]
fn fromstring(py: Python<'_>, xml: &str) -> PyResult<PyElement> {
    let tag = parse_single_tag(xml).map_err(PyValueError::new_err)?;
    let doc = Py::new(py, PyDocument::new(Some(tag)))?;

    Ok(PyElement {
        node_id: 0,
        doc_ref: doc,
    })
}

#[pyfunction]
fn tostring(py: Python<'_>, element: &PyElement) -> PyResult<String> {
    let doc = element.doc_ref.borrow(py);
    let node = doc
        .nodes
        .get(element.node_id)
        .ok_or_else(|| PyIndexError::new_err("node out of bounds"))?;

    let text = node.text.clone().unwrap_or_default();
    Ok(format!("<{}>{}</{}>", node.tag, text, node.tag))
}

#[pymodule]
fn _rust_etree(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyDocument>()?;
    m.add_class::<PyElement>()?;
    m.add_function(wrap_pyfunction!(fromstring, m)?)?;
    m.add_function(wrap_pyfunction!(tostring, m)?)?;
    Ok(())
}
