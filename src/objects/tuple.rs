// Copyright (c) 2015 Daniel Grunwald
//
// Permission is hereby granted, free of charge, to any person obtaining a copy of this
// software and associated documentation files (the "Software"), to deal in the Software
// without restriction, including without limitation the rights to use, copy, modify, merge,
// publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons
// to whom the Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all copies or
// substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
// INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
// PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE
// FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
// OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use python::{Python, PythonObject, ToPythonPointer, PyDrop};
use err::{self, PyErr, PyResult};
use super::object::PyObject;
use super::exc;
use ffi::{self, Py_ssize_t};
use conversion::{FromPyObject, ToPyObject};
use std::slice;

/// Represents a Python tuple object.
pub struct PyTuple(PyObject);

pyobject_newtype!(PyTuple, PyTuple_Check, PyTuple_Type);

impl PyTuple {
    /// Construct a new tuple with the given elements.
    pub fn new(py: Python, elements: &[PyObject]) -> PyTuple {
        unsafe {
            let len = elements.len();
            let ptr = ffi::PyTuple_New(len as Py_ssize_t);
            let t = err::result_cast_from_owned_ptr::<PyTuple>(py, ptr).unwrap();
            for (i, e) in elements.iter().enumerate() {
                ffi::PyTuple_SetItem(ptr, i as Py_ssize_t, e.steal_ptr(py));
            }
            t
        }
    }

    /// Retrieves the empty tuple.
    pub fn empty(py: Python) -> PyTuple {
        unsafe {
            err::result_cast_from_owned_ptr::<PyTuple>(py, ffi::PyTuple_New(0)).unwrap()
        }
    }

    /// Gets the length of the tuple.
    #[inline]
    pub fn len(&self, _py: Python) -> usize {
        unsafe {
            // non-negative Py_ssize_t should always fit into Rust uint
            ffi::PyTuple_GET_SIZE(self.0.as_ptr()) as usize
        }
    }

    /// Gets the item at the specified index.
    ///
    /// Panics if the index is out of range.
    pub fn get_item(&self, py: Python, index: usize) -> PyObject {
        // TODO: reconsider whether we should panic
        // It's quite inconsistent that this method takes `Python` when `len()` does not.
        assert!(index < self.len(py));
        unsafe {
            PyObject::from_borrowed_ptr(py, ffi::PyTuple_GET_ITEM(self.0.as_ptr(), index as Py_ssize_t))
        }
    }

    #[inline]
    #[cfg(feature="nightly")] // needs unsafe_no_drop_flag
    pub fn as_slice<'a>(&'a self) -> &'a [PyObject] {
        // This is safe because PyObject has the same memory layout as *mut ffi::PyObject,
        // and because tuples are immutable.
        // (We don't even need a Python token, thanks to immutability)
        unsafe {
            let ptr = self.0.as_ptr() as *mut ffi::PyTupleObject;
            PyObject::borrow_from_owned_ptr_slice(
                slice::from_raw_parts(
                    (*ptr).ob_item.as_ptr(),
                    ffi::PyTuple_GET_SIZE(self.0.as_ptr()) as usize
                ))
        }
    }

    #[inline]
    pub fn iter<'a, 'p>(&'a self, py: Python<'p>) -> PyTupleIterator<'a, 'p> {
        PyTupleIterator {
            py: py,
            tuple: self,
            index: 0,
            end: self.len(py)
        }
    }
}

#[cfg(feature="nightly")] // needs unsafe_no_drop_flag
impl ::std::ops::Index<usize> for PyTuple {
    type Output = PyObject;

    #[inline]
    fn index(&self, index: usize) -> &PyObject {
        &self.as_slice()[index]
    }
}

#[cfg(feature="nightly")] // needs unsafe_no_drop_flag
impl <'a> IntoIterator for &'a PyTuple {
    type Item = &'a PyObject;
    type IntoIter = slice::Iter<'a, PyObject>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

/// Used by `PyTuple::iter()`.
pub struct PyTupleIterator<'a, 'p> {
    py: Python<'p>,
    tuple: &'a PyTuple,
    index: usize,
    end: usize
}

impl <'a, 'p> Iterator for PyTupleIterator<'a, 'p> {
    type Item = PyObject;

    #[inline]
    fn next(&mut self) -> Option<PyObject> {
        if self.index < self.end {
            let item = self.tuple.get_item(self.py, self.index);
            self.index += 1;
            Some(item)
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len(), Some(self.len()))
    }
}

impl <'a, 'p> ExactSizeIterator for PyTupleIterator<'a, 'p> {
    #[inline]
    fn len(&self) -> usize {
        self.end - self.index
    }
}

fn wrong_tuple_length(py: Python, t: &PyTuple, expected_length: usize) -> PyErr {
    let msg = format!("Expected tuple of length {}, but got tuple of length {}.", expected_length, t.len(py));
    PyErr::new_lazy_init(py.get_type::<exc::ValueError>(), Some(msg.to_py_object(py).into_object()))
}

macro_rules! tuple_conversion ({$length:expr,$(($refN:ident, $n:tt, $T:ident)),+} => {
    impl <$($T: ToPyObject),+> ToPyObject for ($($T,)+) {
        type ObjectType = PyTuple;

        fn to_py_object(&self, py: Python) -> PyTuple {
            PyTuple::new(py, &[
                $(py_coerce_expr!(self.$n.to_py_object(py)).into_object(),)+
            ])
        }

        fn into_py_object(self, py: Python) -> PyTuple {
            PyTuple::new(py, &[
                $(py_coerce_expr!(self.$n.into_py_object(py)).into_object(),)+
            ])
        }
    }

    #[cfg(not(feature="nightly"))]
    impl <'s, $($T: for <'a> FromPyObject<'a>),+> FromPyObject<'s> for ($($T,)+) {
        fn extract(py: Python, obj: &'s PyObject) -> PyResult<Self> {
            let t = try!(obj.cast_as::<PyTuple>(py));
            if t.len(py) == $length {
                Ok((
                    $( py_coerce_expr! {{
                        let item = t.get_item(py, $n);
                        let r = try!(item.extract::<$T>(py));
                        item.release_ref(py);
                        r
                    }} ,)+
                ))
            } else {
                Err(wrong_tuple_length(py, t, $length))
            }
        }
    }

    #[cfg(feature="nightly")]
    impl <'s, $($T: FromPyObject<'s>),+> FromPyObject<'s> for ($($T,)+) {
        fn extract(py: Python, obj: &'s PyObject) -> PyResult<Self> {
            let t = try!(obj.cast_as::<PyTuple>(py));
            let slice = t.as_slice();
            if slice.len() == $length {
                Ok((
                    $( try!(slice[$n].extract::<$T>(py)), )+
                ))
            } else {
                Err(wrong_tuple_length(py, t, $length))
            }
        }
    }
});

tuple_conversion!(1, (ref0, 0, A));
tuple_conversion!(2, (ref0, 0, A), (ref1, 1, B));
tuple_conversion!(3, (ref0, 0, A), (ref1, 1, B), (ref2, 2, C));
tuple_conversion!(4, (ref0, 0, A), (ref1, 1, B), (ref2, 2, C), (ref3, 3, D));
tuple_conversion!(5, (ref0, 0, A), (ref1, 1, B), (ref2, 2, C), (ref3, 3, D),
  (ref4, 4, E));
tuple_conversion!(6, (ref0, 0, A), (ref1, 1, B), (ref2, 2, C), (ref3, 3, D),
  (ref4, 4, E), (ref5, 5, F));
tuple_conversion!(7, (ref0, 0, A), (ref1, 1, B), (ref2, 2, C), (ref3, 3, D),
  (ref4, 4, E), (ref5, 5, F), (ref6, 6, G));
tuple_conversion!(8, (ref0, 0, A), (ref1, 1, B), (ref2, 2, C), (ref3, 3, D),
  (ref4, 4, E), (ref5, 5, F), (ref6, 6, G), (ref7, 7, H));
tuple_conversion!(9, (ref0, 0, A), (ref1, 1, B), (ref2, 2, C), (ref3, 3, D),
  (ref4, 4, E), (ref5, 5, F), (ref6, 6, G), (ref7, 7, H), (ref8, 8, I));

// Empty tuple:

/// An empty struct that represents the empty argument list.
/// Corresponds to the empty tuple `()` in Python.
///
/// # Example
/// ```
/// let gil_guard = cpython::Python::acquire_gil();
/// let py = gil_guard.python();
/// let os = py.import("os").unwrap();
/// let pid = os.call(py, "get_pid", cpython::NoArgs, None);
/// ```
#[derive(Copy, Clone, Debug)]
pub struct NoArgs;

/// Converts `NoArgs` to an empty Python tuple.
impl ToPyObject for NoArgs {
    type ObjectType = PyTuple;

    fn to_py_object(&self, py: Python) -> PyTuple {
        PyTuple::empty(py)
    }
}

/// Returns `Ok(NoArgs)` if the input is an empty Python tuple.
/// Otherwise, returns an error.
extract!(obj to NoArgs; py => {
    let t = try!(obj.cast_as::<PyTuple>(py));
    if t.len(py) == 0 {
        Ok(NoArgs)
    } else {
        Err(wrong_tuple_length(py, t, 0))
    }
});



#[cfg(test)]
mod test {
    use python::{Python, PythonObject};
    use conversion::ToPyObject;

    #[test]
    fn test_len() {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let tuple = (1, 2, 3).to_py_object(py);
        assert_eq!(3, tuple.len(py));
        assert_eq!((1, 2, 3), tuple.into_object().extract(py).unwrap());
    }
}

