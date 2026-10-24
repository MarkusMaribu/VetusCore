/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::dyld::{export_c_func, FunctionExports, HostDylib};
use crate::mem::{ConstPtr, MutPtr};
use crate::Environment;
use rusqlite::{ffi, Connection};
use std::collections::HashMap;
use std::ffi::CStr;
use std::sync::{Arc, Mutex};

// SQLite result codes per https://www.sqlite.org/rescode.html
const SQLITE_OK: u32 = 0;
const SQLITE_ERROR: u32 = 1;
const SQLITE_ROW: u32 = 100;
const SQLITE_DONE: u32 = 101;
const SQLITE_MISUSE: u32 = 21;
const SQLITE_BUSY: u32 = 5;

lazy_static::lazy_static! {
    static ref SQLITE_CONNECTIONS: Mutex<HashMap<u32, Arc<Mutex<Connection>>>> = Mutex::new(HashMap::new());
    static ref SQLITE_STATEMENTS: Mutex<HashMap<u32, StmtEntry>> = Mutex::new(HashMap::new());
    static ref NEXT_HANDLE: Mutex<u32> = Mutex::new(0x8000_0000);
    static ref LAST_ERROR: Mutex<HashMap<u32, String>> = Mutex::new(HashMap::new());
    static ref ERROR_MESSAGE_PTRS: Mutex<HashMap<(u32, bool), u32>> = Mutex::new(HashMap::new());
}

struct StmtEntry {
    db_handle: u32,
    native: NativeStatement,
    columns: Vec<ColumnValue>,
    /// Column names as reported by SQLite for the current prepared statement.
    /// Available immediately after prepare, before the first row is stepped.
    column_names: Vec<String>,
    /// Guest-side cached pointers for `sqlite3_column_name`. SQLite documents
    /// that the returned pointer is valid until the statement is finalised
    /// (or the next call to `sqlite3_column_name` for the same column) so we
    /// keep one guest allocation per column index, reused across calls.
    column_name_ptrs: HashMap<i32, u32>,
    column_name16_ptrs: HashMap<i32, u32>,
    column_text16_ptrs: HashMap<i32, u32>,
}

/// An actual SQLite VM, kept until finalize rather than rebuilt on each step.
/// The connection stays alive for statements after sqlite3_close_v2, and its
/// mutex serializes all native SQLite access. The opaque pointer never leaves
/// this module or crosses into guest memory.
struct NativeStatement {
    pointer: usize,
    connection: Arc<Mutex<Connection>>,
}

impl NativeStatement {
    fn as_ptr(&self) -> *mut ffi::sqlite3_stmt {
        self.pointer as *mut ffi::sqlite3_stmt
    }

    fn finalize(&mut self) -> u32 {
        let _connection = self.connection.lock().unwrap();
        let pointer = std::mem::take(&mut self.pointer);
        unsafe { ffi::sqlite3_finalize(pointer as *mut ffi::sqlite3_stmt) as u32 }
    }
}

impl Drop for NativeStatement {
    fn drop(&mut self) {
        if self.pointer != 0 {
            self.finalize();
        }
    }
}

fn with_statement(stmt: u32, action: impl FnOnce(&mut StmtEntry) -> u32) -> u32 {
    let mut statements = SQLITE_STATEMENTS.lock().unwrap();
    let Some(entry) = statements.get_mut(&stmt) else {
        return SQLITE_MISUSE;
    };
    let connection = Arc::clone(&entry.native.connection);
    let _connection = connection.lock().unwrap();
    action(entry)
}

fn bind_value(stmt: u32, index: i32, value: BindValue) -> u32 {
    with_statement(stmt, |entry| unsafe {
        let pointer = entry.native.as_ptr();
        (match value {
            BindValue::Int(value) => ffi::sqlite3_bind_int(pointer, index, value),
            BindValue::Int64(value) => ffi::sqlite3_bind_int64(pointer, index, value),
            BindValue::Double(value) => ffi::sqlite3_bind_double(pointer, index, value),
            BindValue::Text(value) => ffi::sqlite3_bind_text(
                pointer, index, value.as_ptr().cast(), value.len() as i32, ffi::SQLITE_TRANSIENT(),
            ),
            BindValue::Blob(value) => ffi::sqlite3_bind_blob(
                pointer, index, value.as_ptr().cast(), value.len() as i32, ffi::SQLITE_TRANSIENT(),
            ),
            BindValue::Null => ffi::sqlite3_bind_null(pointer, index),
        }) as u32
    })
}

unsafe fn statement_column_names(pointer: *mut ffi::sqlite3_stmt) -> Vec<String> {
    (0..ffi::sqlite3_column_count(pointer))
        .map(|column| CStr::from_ptr(ffi::sqlite3_column_name(pointer, column)).to_string_lossy().into_owned())
        .collect()
}

#[derive(Clone)]
enum BindValue {
    Int(i32),
    Int64(i64),
    Double(f64),
    Text(String),
    Blob(Vec<u8>),
    Null,
}

#[derive(Clone)]
enum ColumnValue {
    Int(i64),
    Double(f64),
    Text(String),
    Blob(Vec<u8>),
    Null,
}

fn read_cstring(env: &Environment, ptr: u32) -> String {
    if ptr == 0 {
        return String::new();
    }
    let mut bytes = Vec::new();
    let mut addr = ptr;
    loop {
        let p: ConstPtr<u8> = ConstPtr::from_bits(addr);
        let b: u8 = env.mem.read(p);
        if b == 0 {
            break;
        }
        bytes.push(b);
        addr += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn alloc_handle() -> u32 {
    let mut next = NEXT_HANDLE.lock().unwrap();
    let h = *next;
    *next += 4;
    h
}

fn set_error(db_handle: u32, msg: String) {
    LAST_ERROR.lock().unwrap().insert(db_handle, msg);
}

fn alloc_utf16(env: &mut Environment, text: &str) -> u32 {
    let units: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let pointer = env.mem.alloc((units.len() * 2).try_into().unwrap()).cast::<u16>();
    for (i, unit) in units.into_iter().enumerate() {
        env.mem.write(pointer + i as u32, unit);
    }
    pointer.to_bits()
}

fn read_utf16(env: &Environment, pointer: u32, byte_length: i32) -> Vec<u16> {
    let mut units = Vec::new();
    let pointer = ConstPtr::<u16>::from_bits(pointer);
    for i in 0.. {
        if byte_length >= 0 && i >= byte_length as u32 / 2 { break; }
        let unit = env.mem.read(pointer + i);
        if byte_length < 0 && unit == 0 { break; }
        units.push(unit);
    }
    units
}

fn free_cached_pointers(env: &mut Environment, pointers: &mut HashMap<i32, u32>) {
    for (_, pointer) in pointers.drain() {
        env.mem.free(crate::mem::MutVoidPtr::from_bits(pointer));
    }
}

fn free_error_pointers(env: &mut Environment, db: u32) {
    let mut pointers = ERROR_MESSAGE_PTRS.lock().unwrap();
    for utf16 in [false, true] {
        if let Some(pointer) = pointers.remove(&(db, utf16)) {
            env.mem.free(crate::mem::MutVoidPtr::from_bits(pointer));
        }
    }
}

// ---------- sqlite3_open ----------
pub fn sqlite3_open(env: &mut Environment, filename_ptr: u32, pp_db: u32) -> u32 {
    let filename = read_cstring(env, filename_ptr);

    let path = if filename == ":memory:" {
        ":memory:".to_string()
    } else {
        let safe_name = filename
            .replace('/', "_")
            .replace('\\', "_")
            .replace(':', "_");

        let app_ns = std::env::var("TOUCHHLE_SQLITE_NAMESPACE")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| env.bundle.bundle_identifier().to_string())
            .replace('/', "_")
            .replace('\\', "_")
            .replace(':', "_")
            .replace(' ', "_");

        let dir = crate::paths::user_data_base_path().join("touchHLE_sqlite");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log!("libsqlite3: failed to create sqlite dir {:?}: {}", dir, e);
        }

        dir.join(format!("{}_{}", app_ns, safe_name))
            .to_string_lossy()
            .into_owned()
    };

    log!(
        "libsqlite3: sqlite3_open requested {:?} => {:?}",
        filename,
        path
    );

    match Connection::open(&path) {
        Ok(conn) => {
            let handle = alloc_handle();
            SQLITE_CONNECTIONS.lock().unwrap().insert(handle, Arc::new(Mutex::new(conn)));
            let p: MutPtr<u32> = MutPtr::from_bits(pp_db);
            env.mem.write(p, handle);
            SQLITE_OK
        }
        Err(e) => {
            log!("libsqlite3: sqlite3_open failed for {:?}: {}", path, e);
            let p: MutPtr<u32> = MutPtr::from_bits(pp_db);
            env.mem.write(p, 0u32);
            SQLITE_ERROR
        }
    }
}

// ---------- sqlite3_open_v2 ----------
pub fn sqlite3_open_v2(
    env: &mut Environment,
    filename_ptr: u32,
    pp_db: u32,
    _flags: u32,
    _vfs: u32,
) -> u32 {
    sqlite3_open(env, filename_ptr, pp_db)
}

// ---------- sqlite3_close ----------
pub fn sqlite3_close(env: &mut Environment, p_db: u32) -> u32 {
    let mut handles = SQLITE_CONNECTIONS.lock().unwrap();
    if handles.get(&p_db).is_some_and(|conn| Arc::strong_count(conn) > 1) {
        return SQLITE_BUSY;
    }
    if handles.remove(&p_db).is_some() {
        LAST_ERROR.lock().unwrap().remove(&p_db);
        free_error_pointers(env, p_db);
        SQLITE_OK
    } else {
        SQLITE_ERROR
    }
}

pub fn sqlite3_close_v2(env: &mut Environment, p_db: u32) -> u32 {
    let mut handles = SQLITE_CONNECTIONS.lock().unwrap();
    if handles.remove(&p_db).is_some() {
        LAST_ERROR.lock().unwrap().remove(&p_db);
        free_error_pointers(env, p_db);
        SQLITE_OK
    } else {
        SQLITE_ERROR
    }
}

// ---------- sqlite3_exec ----------
pub fn sqlite3_exec(
    env: &mut Environment,
    p_db: u32,
    sql_ptr: u32,
    _callback: u32,
    _user_data: u32,
    err_msg_ptr: u32,
) -> u32 {
    let sql = read_cstring(env, sql_ptr);
    log!("libsqlite3: sqlite3_exec: {}", &sql[..sql.len().min(120)]);

    let handles = SQLITE_CONNECTIONS.lock().unwrap();
    let conn = match handles.get(&p_db) {
        Some(c) => c,
        None => {
            set_error(p_db, "no such connection".into());
            return SQLITE_ERROR;
        }
    };

    let conn = conn.lock().unwrap();
    match conn.execute_batch(&sql) {
        Ok(_) => {
            if err_msg_ptr != 0 {
                let p: MutPtr<u32> = MutPtr::from_bits(err_msg_ptr);
                env.mem.write(p, 0u32);
            }
            SQLITE_OK
        }
        Err(e) => {
            let msg = format!("{}", e);
            log!("libsqlite3: sqlite3_exec error: {}", msg);
            set_error(p_db, msg);
            if err_msg_ptr != 0 {
                let p: MutPtr<u32> = MutPtr::from_bits(err_msg_ptr);
                env.mem.write(p, 0u32);
            }
            SQLITE_ERROR
        }
    }
}

// ---------- sqlite3_prepare (legacy V1) ----------
//
// Per Apple/SQLite documentation
// (<https://www.sqlite.org/c3ref/prepare.html>), `sqlite3_prepare()` is the
// deprecated predecessor of `sqlite3_prepare_v2()`. They share an identical
// C signature; the only documented difference is that statements created with
// V1 do not automatically re-prepare themselves after a schema change. For
// HLE purposes this is invisible to the caller, so we route through the
// existing V2 implementation. (Apps such as `HitNRun` link the legacy
// variant.)
pub fn sqlite3_prepare(
    env: &mut Environment,
    p_db: u32,
    sql_ptr: u32,
    n_byte: i32,
    pp_stmt: u32,
    pp_tail: u32,
) -> u32 {
    sqlite3_prepare_v2(env, p_db, sql_ptr, n_byte, pp_stmt, pp_tail)
}

// ---------- sqlite3_prepare_v2 ----------
pub fn sqlite3_prepare_v2(
    env: &mut Environment,
    p_db: u32,
    sql_ptr: u32,
    n_byte: i32,
    pp_stmt: u32,
    pp_tail: u32,
) -> u32 {
    if pp_stmt != 0 {
        env.mem.write(MutPtr::<u32>::from_bits(pp_stmt), 0);
    }
    let mut sql = if n_byte < 0 {
        read_cstring(env, sql_ptr).into_bytes()
    } else {
        env.mem.bytes_at(ConstPtr::from_bits(sql_ptr), n_byte as u32).to_vec()
    };
    sql.push(0);
    let connection = {
        let handles = SQLITE_CONNECTIONS.lock().unwrap();
        let Some(connection) = handles.get(&p_db) else {
            set_error(p_db, "no such connection".into());
            return SQLITE_MISUSE;
        };
        Arc::clone(connection)
    };
    let conn = connection.lock().unwrap();
    let mut pointer = std::ptr::null_mut();
    let mut tail = std::ptr::null();
    let result = unsafe {
        ffi::sqlite3_prepare_v2(conn.handle(), sql.as_ptr().cast(), n_byte, &mut pointer, &mut tail)
    } as u32;
    if pp_tail != 0 && !tail.is_null() {
        let offset = unsafe { tail.offset_from(sql.as_ptr().cast()) } as u32;
        env.mem.write(MutPtr::<u32>::from_bits(pp_tail), sql_ptr + offset);
    }
    if result != SQLITE_OK {
        let message = unsafe { CStr::from_ptr(ffi::sqlite3_errmsg(conn.handle())) }
            .to_string_lossy().into_owned();
        set_error(p_db, message);
        return result;
    }
    // Empty/comment-only input succeeds without producing a statement.
    if pointer.is_null() {
        return SQLITE_OK;
    }
    let column_names = unsafe { statement_column_names(pointer) };
    drop(conn);
    let stmt_handle = alloc_handle();
    SQLITE_STATEMENTS.lock().unwrap().insert(stmt_handle, StmtEntry {
        db_handle: p_db,
        native: NativeStatement { pointer: pointer as usize, connection },
        columns: Vec::new(),
        column_names,
        column_name_ptrs: HashMap::new(),
        column_name16_ptrs: HashMap::new(),
        column_text16_ptrs: HashMap::new(),
    });
    if pp_stmt != 0 {
        env.mem.write(MutPtr::<u32>::from_bits(pp_stmt), stmt_handle);
    }
    SQLITE_OK
}

// ---------- sqlite3_bind_int ----------
pub fn sqlite3_bind_int(_env: &mut Environment, stmt: u32, index: i32, value: i32) -> u32 {
    bind_value(stmt, index, BindValue::Int(value))
}

pub fn sqlite3_bind_int64(_env: &mut Environment, stmt: u32, index: i32, value: i64) -> u32 {
    bind_value(stmt, index, BindValue::Int64(value))
}

pub fn sqlite3_bind_double(_env: &mut Environment, stmt: u32, index: i32, value: f64) -> u32 {
    bind_value(stmt, index, BindValue::Double(value))
}

// ---------- sqlite3_bind_text ----------
pub fn sqlite3_bind_text(
    env: &mut Environment,
    stmt: u32,
    index: i32,
    text_ptr: u32,
    n_bytes: i32,
    _destructor: u32,
) -> u32 {
    let text = if text_ptr == 0 {
        String::new()
    } else if n_bytes < 0 {
        read_cstring(env, text_ptr)
    } else {
        let mut bytes = Vec::with_capacity(n_bytes as usize);
        for i in 0..(n_bytes as u32) {
            let p: ConstPtr<u8> = ConstPtr::from_bits(text_ptr + i);
            bytes.push(env.mem.read(p));
        }
        String::from_utf8_lossy(&bytes).into_owned()
    };

    if text_ptr == 0 { bind_value(stmt, index, BindValue::Null) }
    else { bind_value(stmt, index, BindValue::Text(text)) }
}

// ---------- sqlite3_bind_blob ----------
pub fn sqlite3_bind_blob(
    env: &mut Environment,
    stmt: u32,
    index: i32,
    blob_ptr: u32,
    n_bytes: i32,
    _destructor: u32,
) -> u32 {
    let blob = if blob_ptr == 0 || n_bytes <= 0 {
        Vec::new()
    } else {
        let mut bytes = Vec::with_capacity(n_bytes as usize);
        for i in 0..(n_bytes as u32) {
            let p: ConstPtr<u8> = ConstPtr::from_bits(blob_ptr + i);
            bytes.push(env.mem.read(p));
        }
        bytes
    };

    if blob_ptr == 0 { bind_value(stmt, index, BindValue::Null) }
    else { bind_value(stmt, index, BindValue::Blob(blob)) }
}

pub fn sqlite3_bind_null(_env: &mut Environment, stmt: u32, index: i32) -> u32 {
    bind_value(stmt, index, BindValue::Null)
}

// Native SQLite consumes UTF-16 in native byte order, honoring a leading BOM.
// Copy the guest buffer because a host SQLite pointer must never reference it.
// https://www.sqlite.org/c3ref/bind_blob.html
pub fn sqlite3_bind_text16(
    env: &mut Environment, stmt: u32, index: i32, text: u32, n_bytes: i32,
    _destructor: u32,
) -> u32 {
    if text == 0 { return sqlite3_bind_null(env, stmt, index); }
    let units = read_utf16(env, text, n_bytes);
    with_statement(stmt, |entry| unsafe {
        ffi::sqlite3_bind_text16(entry.native.as_ptr(), index, units.as_ptr().cast(),
            (units.len() * 2) as i32, ffi::SQLITE_TRANSIENT()) as u32
    })
}

pub fn sqlite3_bind_parameter_index(env: &mut Environment, stmt: u32, name: u32) -> i32 {
    let mut name = read_cstring(env, name).into_bytes();
    name.push(0);
    with_statement(stmt, |entry| unsafe {
        ffi::sqlite3_bind_parameter_index(entry.native.as_ptr(), name.as_ptr().cast()) as u32
    }) as i32
}

// ---------- sqlite3_step ----------
// https://www.sqlite.org/c3ref/step.html: step the existing VM once, keeping
// its cursor, transaction, bindings and SQLite's own error/reset semantics.
pub fn sqlite3_step(env: &mut Environment, stmt_handle: u32) -> u32 {
    with_statement(stmt_handle, |entry| unsafe {
        free_cached_pointers(env, &mut entry.column_text16_ptrs);
        let pointer = entry.native.as_ptr();
        let result = ffi::sqlite3_step(pointer) as u32;
        entry.columns.clear();
        let names = statement_column_names(pointer);
        if entry.column_names != names {
            free_cached_pointers(env, &mut entry.column_name_ptrs);
            free_cached_pointers(env, &mut entry.column_name16_ptrs);
            entry.column_names = names;
        }
        if result == SQLITE_ROW {
            for column in 0..ffi::sqlite3_column_count(pointer) {
                let value = match ffi::sqlite3_column_type(pointer, column) {
                    ffi::SQLITE_INTEGER => ColumnValue::Int(ffi::sqlite3_column_int64(pointer, column)),
                    ffi::SQLITE_FLOAT => ColumnValue::Double(ffi::sqlite3_column_double(pointer, column)),
                    ffi::SQLITE_TEXT => {
                        let text = ffi::sqlite3_column_text(pointer, column);
                        let len = ffi::sqlite3_column_bytes(pointer, column) as usize;
                        ColumnValue::Text(String::from_utf8_lossy(std::slice::from_raw_parts(text, len)).into_owned())
                    }
                    ffi::SQLITE_BLOB => {
                        let blob = ffi::sqlite3_column_blob(pointer, column);
                        let len = ffi::sqlite3_column_bytes(pointer, column) as usize;
                        ColumnValue::Blob(if len == 0 { Vec::new() } else {
                            std::slice::from_raw_parts(blob.cast::<u8>(), len).to_vec()
                        })
                    }
                    _ => ColumnValue::Null,
                };
                entry.columns.push(value);
            }
        } else if result != SQLITE_DONE {
            let db = ffi::sqlite3_db_handle(pointer);
            let message = CStr::from_ptr(ffi::sqlite3_errmsg(db)).to_string_lossy().into_owned();
            set_error(entry.db_handle, message);
        }
        result
    })
}

pub fn sqlite3_reset(env: &mut Environment, stmt_handle: u32) -> u32 {
    with_statement(stmt_handle, |entry| {
        free_cached_pointers(env, &mut entry.column_text16_ptrs);
        entry.columns.clear();
        unsafe { ffi::sqlite3_reset(entry.native.as_ptr()) as u32 }
    })
}

pub fn sqlite3_clear_bindings(_env: &mut Environment, stmt_handle: u32) -> u32 {
    with_statement(stmt_handle, |entry| unsafe {
        ffi::sqlite3_clear_bindings(entry.native.as_ptr()) as u32
    })
}

pub fn sqlite3_finalize(env: &mut Environment, stmt_handle: u32) -> u32 {
    let mut statements = SQLITE_STATEMENTS.lock().unwrap();
    let Some(mut entry) = statements.remove(&stmt_handle) else {
        return SQLITE_OK;
    };
    free_cached_pointers(env, &mut entry.column_name_ptrs);
    free_cached_pointers(env, &mut entry.column_name16_ptrs);
    free_cached_pointers(env, &mut entry.column_text16_ptrs);
    entry.native.finalize()
}

// ---------- sqlite3_column_count ----------
pub fn sqlite3_column_count(_env: &mut Environment, stmt_handle: u32) -> i32 {
    let stmts = SQLITE_STATEMENTS.lock().unwrap();
    match stmts.get(&stmt_handle) {
        Some(entry) => entry.column_names.len() as i32,
        None => 0,
    }
}

// ---------- sqlite3_column_int ----------
pub fn sqlite3_column_int(_env: &mut Environment, stmt_handle: u32, col: i32) -> i32 {
    let stmts = SQLITE_STATEMENTS.lock().unwrap();
    match stmts.get(&stmt_handle) {
        Some(entry) => match entry.columns.get(col as usize) {
            Some(ColumnValue::Int(v)) => *v as i32,
            Some(ColumnValue::Double(v)) => *v as i32,
            Some(ColumnValue::Text(v)) => v.parse::<i32>().unwrap_or(0),
            _ => 0,
        },
        None => 0,
    }
}

// ---------- sqlite3_column_int64 ----------
pub fn sqlite3_column_int64(_env: &mut Environment, stmt_handle: u32, col: i32) -> i64 {
    let stmts = SQLITE_STATEMENTS.lock().unwrap();
    match stmts.get(&stmt_handle) {
        Some(entry) => match entry.columns.get(col as usize) {
            Some(ColumnValue::Int(v)) => *v,
            Some(ColumnValue::Double(v)) => *v as i64,
            Some(ColumnValue::Text(v)) => v.parse::<i64>().unwrap_or(0),
            _ => 0,
        },
        None => 0,
    }
}

// ---------- sqlite3_column_double ----------
pub fn sqlite3_column_double(_env: &mut Environment, stmt_handle: u32, col: i32) -> f64 {
    let stmts = SQLITE_STATEMENTS.lock().unwrap();
    match stmts.get(&stmt_handle) {
        Some(entry) => match entry.columns.get(col as usize) {
            Some(ColumnValue::Double(v)) => *v,
            Some(ColumnValue::Int(v)) => *v as f64,
            Some(ColumnValue::Text(v)) => v.parse::<f64>().unwrap_or(0.0),
            _ => 0.0,
        },
        None => 0.0,
    }
}

// ---------- sqlite3_column_text ----------
// Returns a pointer to a static/leaked string in guest memory.
// This is a simplification - real SQLite returns pointer valid until next step/finalize.
pub fn sqlite3_column_text(env: &mut Environment, stmt_handle: u32, col: i32) -> u32 {
    let text = {
        let stmts = SQLITE_STATEMENTS.lock().unwrap();
        match stmts.get(&stmt_handle) {
            Some(entry) => match entry.columns.get(col as usize) {
                Some(ColumnValue::Text(v)) => v.clone(),
                Some(ColumnValue::Int(v)) => format!("{}", v),
                Some(ColumnValue::Double(v)) => format!("{}", v),
                _ => return 0,
            },
            None => return 0,
        }
    };

    // Allocate in guest memory
    let len = text.len() + 1; // +1 for null terminator
    let guest_ptr = env.mem.alloc(len.try_into().unwrap());
    let base = guest_ptr.to_bits();
    for (i, b) in text.as_bytes().iter().enumerate() {
        let p: MutPtr<u8> = MutPtr::from_bits(base + i as u32);
        env.mem.write(p, *b);
    }
    let p: MutPtr<u8> = MutPtr::from_bits(base + text.len() as u32);
    env.mem.write(p, 0u8);
    base
}

// ---------- sqlite3_column_bytes ----------
pub fn sqlite3_column_bytes(_env: &mut Environment, stmt_handle: u32, col: i32) -> i32 {
    with_statement(stmt_handle, |entry| unsafe {
        ffi::sqlite3_column_bytes(entry.native.as_ptr(), col) as u32
    }) as i32
}

pub fn sqlite3_column_bytes16(_env: &mut Environment, stmt: u32, col: i32) -> i32 {
    with_statement(stmt, |entry| unsafe {
        ffi::sqlite3_column_bytes16(entry.native.as_ptr(), col) as u32
    }) as i32
}

// Keep guest UTF-16 result buffers until step/reset/finalize, just like SQLite.
// https://www.sqlite.org/c3ref/column_blob.html
pub fn sqlite3_column_text16(env: &mut Environment, stmt: u32, col: i32) -> u32 {
    with_statement(stmt, |entry| unsafe {
        if let Some(pointer) = entry.column_text16_ptrs.get(&col) { return *pointer; }
        let source = ffi::sqlite3_column_text16(entry.native.as_ptr(), col);
        if source.is_null() { return 0; }
        let length = ffi::sqlite3_column_bytes16(entry.native.as_ptr(), col) as u32;
        let pointer = env.mem.alloc(length + 2);
        env.mem.bytes_at_mut(pointer.cast(), length)
            .copy_from_slice(std::slice::from_raw_parts(source.cast::<u8>(), length as usize));
        env.mem.write((pointer + length).cast::<u16>(), 0);
        entry.column_text16_ptrs.insert(col, pointer.to_bits());
        pointer.to_bits()
    })
}

// ---------- sqlite3_column_type ----------
pub fn sqlite3_column_type(_env: &mut Environment, stmt_handle: u32, col: i32) -> i32 {
    const SQLITE_INTEGER: i32 = 1;
    const SQLITE_FLOAT: i32 = 2;
    const SQLITE_TEXT: i32 = 3;
    const SQLITE_BLOB: i32 = 4;
    const SQLITE_NULL: i32 = 5;

    let stmts = SQLITE_STATEMENTS.lock().unwrap();
    match stmts.get(&stmt_handle) {
        Some(entry) => match entry.columns.get(col as usize) {
            Some(ColumnValue::Int(_)) => SQLITE_INTEGER,
            Some(ColumnValue::Double(_)) => SQLITE_FLOAT,
            Some(ColumnValue::Text(_)) => SQLITE_TEXT,
            Some(ColumnValue::Blob(_)) => SQLITE_BLOB,
            Some(ColumnValue::Null) | None => SQLITE_NULL,
        },
        None => 5, // SQLITE_NULL
    }
}

// ---------- sqlite3_column_name ----------
//
// Returns a pointer to a NUL-terminated UTF-8 string naming the `col`-th
// result column of the prepared statement. Per SQLite's documentation
// (<https://www.sqlite.org/c3ref/column_name.html>):
//
//   "The returned string pointer is valid until either the prepared
//   statement is destroyed by sqlite3_finalize() or until the statement is
//   automatically reprepared by the first call to sqlite3_step() for a
//   particular run or until the next call to sqlite3_column_name() or
//   sqlite3_column_name16() on the same column."
//
// We satisfy this by keeping one guest-side allocation per (stmt, col), so
// repeated calls return the same pointer.
pub fn sqlite3_column_name(env: &mut Environment, stmt_handle: u32, col: i32) -> u32 {
    // Look up name and any existing guest pointer under the lock.
    let (name, cached) = {
        let stmts = SQLITE_STATEMENTS.lock().unwrap();
        match stmts.get(&stmt_handle) {
            Some(entry) => {
                let name = entry
                    .column_names
                    .get(col as usize)
                    .cloned()
                    .unwrap_or_default();
                (name, entry.column_name_ptrs.get(&col).copied())
            }
            None => return 0,
        }
    };

    if let Some(p) = cached {
        // Cached entry from a previous call. The SQLite contract
        // guarantees the pointer is stable until the next step/finalize.
        return p;
    }

    let bytes = name.as_bytes();
    let len = bytes.len() + 1;
    let guest_ptr = env.mem.alloc(len.try_into().unwrap());
    let base = guest_ptr.to_bits();
    for (i, b) in bytes.iter().enumerate() {
        let p: MutPtr<u8> = MutPtr::from_bits(base + i as u32);
        env.mem.write(p, *b);
    }
    let p: MutPtr<u8> = MutPtr::from_bits(base + bytes.len() as u32);
    env.mem.write(p, 0u8);

    // Cache the guest pointer for this column.
    let mut stmts = SQLITE_STATEMENTS.lock().unwrap();
    if let Some(entry) = stmts.get_mut(&stmt_handle) {
        entry.column_name_ptrs.insert(col, base);
    }
    base
}

// https://www.sqlite.org/c3ref/column_name.html
pub fn sqlite3_column_name16(env: &mut Environment, stmt: u32, col: i32) -> u32 {
    let mut statements = SQLITE_STATEMENTS.lock().unwrap();
    let Some(entry) = statements.get_mut(&stmt) else { return 0; };
    if let Some(pointer) = entry.column_name16_ptrs.get(&col) { return *pointer; }
    let Some(name) = entry.column_names.get(col as usize) else { return 0; };
    let pointer = alloc_utf16(env, name);
    entry.column_name16_ptrs.insert(col, pointer);
    pointer
}

// ---------- sqlite3_data_count ----------
//
// Returns the number of columns in the current result row of a prepared
// statement. Per <https://www.sqlite.org/c3ref/data_count.html>, this is
// the run-time analogue of `sqlite3_column_count`: it returns 0 after
// `SQLITE_DONE`, and the same value as `sqlite3_column_count` after
// `SQLITE_ROW`.
pub fn sqlite3_data_count(_env: &mut Environment, stmt_handle: u32) -> i32 {
    let stmts = SQLITE_STATEMENTS.lock().unwrap();
    match stmts.get(&stmt_handle) {
        Some(entry) => entry.columns.len() as i32,
        None => 0,
    }
}

// ---------- sqlite3_column_blob ----------
//
// Returns a pointer to BLOB column data. SQLite docs guarantee the pointer
// is valid until the next step/reset/finalize for that column, so we
// allocate fresh guest memory each call and let the guest reuse it for
// the same row.
pub fn sqlite3_column_blob(env: &mut Environment, stmt_handle: u32, col: i32) -> u32 {
    let blob = {
        let stmts = SQLITE_STATEMENTS.lock().unwrap();
        match stmts.get(&stmt_handle) {
            Some(entry) => match entry.columns.get(col as usize) {
                Some(ColumnValue::Blob(v)) => v.clone(),
                Some(ColumnValue::Text(v)) => v.as_bytes().to_vec(),
                _ => return 0,
            },
            None => return 0,
        }
    };
    if blob.is_empty() {
        // Real SQLite returns a non-NULL pointer to a zero-length blob, but
        // returning 0 here matches what `sqlite3_column_text` does for empty
        // text — the corresponding `sqlite3_column_bytes` will report 0 too.
        return 0;
    }
    let guest_ptr = env.mem.alloc(blob.len().try_into().unwrap());
    let base = guest_ptr.to_bits();
    for (i, b) in blob.iter().enumerate() {
        let p: MutPtr<u8> = MutPtr::from_bits(base + i as u32);
        env.mem.write(p, *b);
    }
    base
}

// ---------- sqlite3_errmsg ----------
// Returns pointer to error string in guest memory
pub fn sqlite3_errmsg(env: &mut Environment, p_db: u32) -> u32 {
    let msg = {
        let errors = LAST_ERROR.lock().unwrap();
        errors
            .get(&p_db)
            .cloned()
            .unwrap_or_else(|| "not an error".to_string())
    };

    let len = msg.len() + 1;
    let guest_ptr = env.mem.alloc(len.try_into().unwrap());
    let base = guest_ptr.to_bits();
    for (i, b) in msg.as_bytes().iter().enumerate() {
        let p: MutPtr<u8> = MutPtr::from_bits(base + i as u32);
        env.mem.write(p, *b);
    }
    let p: MutPtr<u8> = MutPtr::from_bits(base + msg.len() as u32);
    env.mem.write(p, 0u8);
    base
}

pub fn sqlite3_errmsg16(env: &mut Environment, db: u32) -> u32 {
    let message = LAST_ERROR.lock().unwrap().get(&db).cloned()
        .unwrap_or_else(|| "not an error".to_string());
    let pointer = alloc_utf16(env, &message);
    if let Some(old) = ERROR_MESSAGE_PTRS.lock().unwrap().insert((db, true), pointer) {
        env.mem.free(crate::mem::MutVoidPtr::from_bits(old));
    }
    pointer
}

// ---------- sqlite3_changes ----------
pub fn sqlite3_changes(_env: &mut Environment, p_db: u32) -> i32 {
    let handles = SQLITE_CONNECTIONS.lock().unwrap();
    match handles.get(&p_db) {
        Some(conn) => conn.lock().unwrap().changes() as i32,
        None => 0,
    }
}

// ---------- sqlite3_last_insert_rowid ----------
pub fn sqlite3_last_insert_rowid(_env: &mut Environment, p_db: u32) -> i64 {
    let handles = SQLITE_CONNECTIONS.lock().unwrap();
    match handles.get(&p_db) {
        Some(conn) => conn.lock().unwrap().last_insert_rowid(),
        None => 0,
    }
}

// ---------- sqlite3_free ----------
pub fn sqlite3_free(env: &mut Environment, ptr: u32) {
    if ptr == 0 {
        return;
    }
    let mem_ptr = crate::mem::MutVoidPtr::from_bits(ptr);
    env.mem.free(mem_ptr);
}

// ---------- sqlite3_busy_timeout ----------
pub fn sqlite3_busy_timeout(_env: &mut Environment, _p_db: u32, _ms: i32) -> u32 {
    SQLITE_OK
}

// ---------- sqlite3_errcode ----------
pub fn sqlite3_errcode(_env: &mut Environment, _p_db: u32) -> u32 {
    SQLITE_OK
}

// Export all functions
pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(sqlite3_open(_, _)),
    export_c_func!(sqlite3_open_v2(_, _, _, _)),
    export_c_func!(sqlite3_close(_)),
    export_c_func!(sqlite3_close_v2(_)),
    export_c_func!(sqlite3_exec(_, _, _, _, _)),
    export_c_func!(sqlite3_prepare(_, _, _, _, _)),
    export_c_func!(sqlite3_prepare_v2(_, _, _, _, _)),
    export_c_func!(sqlite3_bind_int(_, _, _)),
    export_c_func!(sqlite3_bind_int64(_, _, _)),
    export_c_func!(sqlite3_bind_double(_, _, _)),
    export_c_func!(sqlite3_bind_text(_, _, _, _, _)),
    export_c_func!(sqlite3_bind_text16(_, _, _, _, _)),
    export_c_func!(sqlite3_bind_parameter_index(_, _)),
    export_c_func!(sqlite3_bind_blob(_, _, _, _, _)),
    export_c_func!(sqlite3_bind_null(_, _)),
    export_c_func!(sqlite3_step(_)),
    export_c_func!(sqlite3_reset(_)),
    export_c_func!(sqlite3_clear_bindings(_)),
    export_c_func!(sqlite3_finalize(_)),
    export_c_func!(sqlite3_column_count(_)),
    export_c_func!(sqlite3_column_int(_, _)),
    export_c_func!(sqlite3_column_int64(_, _)),
    export_c_func!(sqlite3_column_double(_, _)),
    export_c_func!(sqlite3_column_text(_, _)),
    export_c_func!(sqlite3_column_text16(_, _)),
    export_c_func!(sqlite3_column_bytes(_, _)),
    export_c_func!(sqlite3_column_bytes16(_, _)),
    export_c_func!(sqlite3_column_type(_, _)),
    export_c_func!(sqlite3_column_name(_, _)),
    export_c_func!(sqlite3_column_name16(_, _)),
    export_c_func!(sqlite3_data_count(_)),
    export_c_func!(sqlite3_column_blob(_, _)),
    export_c_func!(sqlite3_errmsg(_)),
    export_c_func!(sqlite3_errmsg16(_)),
    export_c_func!(sqlite3_changes(_)),
    export_c_func!(sqlite3_last_insert_rowid(_)),
    export_c_func!(sqlite3_free(_)),
    export_c_func!(sqlite3_busy_timeout(_, _)),
    export_c_func!(sqlite3_errcode(_)),
];

pub const DYLIB: HostDylib = HostDylib {
    path: "/usr/lib/libsqlite3.dylib",
    aliases: &["/usr/lib/libsqlite3.0.dylib"],
    class_exports: &[],
    constant_exports: &[],
    function_exports: &[FUNCTIONS],
};

#[cfg(test)]
mod tests {
    use super::*;

    fn prepare(env: &mut Environment, db: u32, sql: &str) -> u32 {
        let text = env.mem.alloc_and_write_cstr(sql.as_bytes());
        let output = env.mem.alloc(4).cast::<u32>();
        assert_eq!(sqlite3_prepare_v2(env, db, text.to_bits(), -1, output.to_bits(), 0), SQLITE_OK);
        let statement = env.mem.read(output);
        env.mem.free(text.cast());
        env.mem.free(output.cast());
        statement
    }

    // Each step advances the same prepared statement; reset restarts it and
    // retains its bound parameters. This also covers multi-row PRAGMA queries.
    // https://www.sqlite.org/c3ref/step.html
    // https://www.sqlite.org/c3ref/reset.html
    #[test]
    fn prepared_statement_advances_rows_and_reset_reuses_bindings() {
        let mut env = crate::environment::touch_test_environment();
        let filename = env.mem.alloc_and_write_cstr(b":memory:");
        let output = env.mem.alloc(4).cast::<u32>();
        assert_eq!(sqlite3_open(&mut env, filename.to_bits(), output.to_bits()), SQLITE_OK);
        let db = env.mem.read(output);
        let statement = prepare(&mut env, db,
            "SELECT 11 AS value WHERE ?1 = 7 UNION ALL SELECT 22 WHERE ?1 = 7");
        assert_eq!(sqlite3_bind_int(&mut env, statement, 1, 7), SQLITE_OK);
        assert_eq!(sqlite3_step(&mut env, statement), SQLITE_ROW);
        assert_eq!(sqlite3_column_int(&mut env, statement, 0), 11);
        assert_eq!(sqlite3_step(&mut env, statement), SQLITE_ROW);
        assert_eq!(sqlite3_column_int(&mut env, statement, 0), 22);
        assert_eq!(sqlite3_step(&mut env, statement), SQLITE_DONE);
        assert_eq!(sqlite3_data_count(&mut env, statement), 0);
        assert_eq!(sqlite3_reset(&mut env, statement), SQLITE_OK);
        assert_eq!(sqlite3_step(&mut env, statement), SQLITE_ROW);
        assert_eq!(sqlite3_column_int(&mut env, statement, 0), 11);
        assert_eq!(sqlite3_finalize(&mut env, statement), SQLITE_OK);
        assert_eq!(sqlite3_close(&mut env, db), SQLITE_OK);
        env.mem.free(filename.cast());
        env.mem.free(output.cast());
    }
}
