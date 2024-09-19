use anyhow::Result;
use std::{
    ffi::{c_char, CString}, mem, ptr
};

#[repr(C)]
pub struct CResult<T> {
    pub value: T,
    pub error: *mut c_char,
    pub len: u32,
}

#[repr(C)]
pub struct CParam {
    pub value: *mut u8,
    pub len: u32,
}

impl <T> CResult<T> {
    pub fn new(value: T) -> Self {
        Self {
            value,
            len: 0,
            error: ptr::null_mut::<c_char>(),
        }
    }
}

pub fn map_result<T>(res: Result<T>) -> CResult<T> {
    match res {
        Ok(v) => CResult::new(v),
        Err(e) => {
            tracing::error!("{}", e);
            CResult {
                value: unsafe { std::mem::zeroed() },
                len: 0,
                error: to_c_str(e.to_string()),
            }
        }
    }
}

pub fn map_result_string(res: Result<String>) -> CResult<*mut c_char> {
    let res = res.map(to_c_str);
    map_result(res)
}

pub fn map_result_bytes(res: Result<Vec<u8>>) -> CResult<*const u8> {
    match res {
        Ok(v) => {
            let (value, len) = to_bytes(v);
            CResult {
                value,
                len,
                error: ptr::null_mut::<c_char>(),
            }
        }
        Err(e) => {
            tracing::error!("{}", e);
            CResult {
                value: unsafe { std::mem::zeroed() },
                len: 0,
                error: to_c_str(e.to_string()),
            }
        }
    }
}

fn to_c_str(s: String) -> *mut c_char {
    CString::new(s).unwrap().into_raw()
}

fn to_bytes(mut b: Vec<u8>) -> (*const u8, u32) {
    b.shrink_to_fit();
    let me = mem::ManuallyDrop::new(b);
    let ptr = me.as_ptr();
    let len = me.len() as u32;
    (ptr, len)
}
