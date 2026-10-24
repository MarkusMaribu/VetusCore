/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#include <stddef.h>
#include <stdint.h>
#include <string.h>

typedef struct sqlite3 sqlite3;
typedef struct sqlite3_stmt sqlite3_stmt;
extern void *dlsym(void *, const char *);
static int (*sqlite3_open)(const char *, sqlite3 **);
static int (*sqlite3_close)(sqlite3 *);
static int (*sqlite3_close_v2)(sqlite3 *);
static int (*sqlite3_exec)(sqlite3 *, const char *, void *, void *, char **);
static int (*sqlite3_prepare_v2)(sqlite3 *, const char *, int, sqlite3_stmt **, const char **);
static int (*sqlite3_bind_int)(sqlite3_stmt *, int, int);
static int (*sqlite3_step)(sqlite3_stmt *);
static int (*sqlite3_reset)(sqlite3_stmt *);
static int (*sqlite3_clear_bindings)(sqlite3_stmt *);
static int (*sqlite3_finalize)(sqlite3_stmt *);
static int (*sqlite3_column_count)(sqlite3_stmt *);
static int (*sqlite3_data_count)(sqlite3_stmt *);
static int (*sqlite3_column_int)(sqlite3_stmt *, int);
static int (*sqlite3_column_type)(sqlite3_stmt *, int);
static int (*sqlite3_column_bytes)(sqlite3_stmt *, int);
static int (*sqlite3_changes)(sqlite3 *);
static const char *(*sqlite3_column_name)(sqlite3_stmt *, int);
static const unsigned char *(*sqlite3_column_text)(sqlite3_stmt *, int);

// Test the HLE implementation consistently. Direct imports can bind to the
// bundled guest SQLite, whose database pointers cannot be passed to host APIs.
static int resolve_sqlite(void) {
#define RESOLVE(name) if (!(name = dlsym((void *)-2, #name))) return 0
  RESOLVE(sqlite3_open);
  RESOLVE(sqlite3_close);
  RESOLVE(sqlite3_close_v2);
  RESOLVE(sqlite3_exec);
  RESOLVE(sqlite3_prepare_v2);
  RESOLVE(sqlite3_bind_int);
  RESOLVE(sqlite3_step);
  RESOLVE(sqlite3_reset);
  RESOLVE(sqlite3_clear_bindings);
  RESOLVE(sqlite3_finalize);
  RESOLVE(sqlite3_column_count);
  RESOLVE(sqlite3_data_count);
  RESOLVE(sqlite3_column_int);
  RESOLVE(sqlite3_column_type);
  RESOLVE(sqlite3_column_bytes);
  RESOLVE(sqlite3_changes);
  RESOLVE(sqlite3_column_name);
  RESOLVE(sqlite3_column_text);
#undef RESOLVE
  return 1;
}

#define CHECK(condition) do { if (!(condition)) { result = -__LINE__; goto cleanup; } } while (0)

// A new step returns the next row of the existing VM. Reset rewinds that VM,
// preserving parameters; clear_bindings clears parameters rather than rows.
// https://www.sqlite.org/c3ref/step.html
// https://www.sqlite.org/c3ref/reset.html
int test_sqlite_cursor_bindings_and_metadata(void) {
  sqlite3 *db = NULL;
  sqlite3_stmt *stmt = NULL;
  int result = 0;
  if (!resolve_sqlite()) return -__LINE__;
  CHECK(sqlite3_open(":memory:", &db) == 0);
  CHECK(sqlite3_prepare_v2(db, "SELECT ?1 AS number UNION ALL SELECT ?1+1", -1, &stmt, NULL) == 0);
  CHECK(sqlite3_column_count(stmt) == 1);
  CHECK(strcmp(sqlite3_column_name(stmt, 0), "number") == 0);
  CHECK(sqlite3_bind_int(stmt, 1, 11) == 0);
  CHECK(sqlite3_step(stmt) == 100 && sqlite3_column_int(stmt, 0) == 11);
  CHECK(sqlite3_step(stmt) == 100 && sqlite3_column_int(stmt, 0) == 12);
  CHECK(sqlite3_step(stmt) == 101);
  CHECK(sqlite3_data_count(stmt) == 0 && sqlite3_column_count(stmt) == 1);
  CHECK(sqlite3_reset(stmt) == 0);
  CHECK(sqlite3_step(stmt) == 100 && sqlite3_column_int(stmt, 0) == 11);
  CHECK(sqlite3_reset(stmt) == 0);
  CHECK(sqlite3_clear_bindings(stmt) == 0);
  CHECK(sqlite3_step(stmt) == 100 && sqlite3_column_type(stmt, 0) == 5);
cleanup:
  sqlite3_finalize(stmt);
  if (db) sqlite3_close(db);
  return result;
}

// Schema enumeration is a real multi-row PRAGMA, as used by SQLite wrappers.
int test_sqlite_pragma_schema_enumeration_terminates(void) {
  sqlite3 *db = NULL;
  sqlite3_stmt *stmt = NULL;
  int result = 0;
  if (!resolve_sqlite()) return -__LINE__;
  CHECK(sqlite3_open(":memory:", &db) == 0);
  CHECK(sqlite3_exec(db, "CREATE TABLE state (key TEXT PRIMARY KEY, value TEXT, counter INTEGER)", NULL, NULL, NULL) == 0);
  CHECK(sqlite3_exec(db, "INSERT INTO state VALUES ('a', 'first', 1), ('b', 'second', 2)", NULL, NULL, NULL) == 0);
  CHECK(sqlite3_changes(db) == 2);
  CHECK(sqlite3_prepare_v2(db, "PRAGMA table_info(state)", -1, &stmt, NULL) == 0);
  const char *names[] = {"key", "value", "counter"};
  for (int row = 0; row < 3; row++) {
    CHECK(sqlite3_step(stmt) == 100);
    CHECK(sqlite3_column_int(stmt, 0) == row);
    CHECK(strcmp((const char *)sqlite3_column_text(stmt, 1), names[row]) == 0);
  }
  CHECK(sqlite3_step(stmt) == 101);
cleanup:
  sqlite3_finalize(stmt);
  if (db) sqlite3_close(db);
  return result;
}

// Unfinalized statements keep the database alive, including after close_v2.
// https://www.sqlite.org/c3ref/close.html
int test_sqlite_statement_owns_connection_until_finalize(void) {
  sqlite3 *db = NULL;
  sqlite3_stmt *stmt = NULL;
  int result = 0;
  if (!resolve_sqlite()) return -__LINE__;
  CHECK(sqlite3_open(":memory:", &db) == 0);
  CHECK(sqlite3_prepare_v2(db, "SELECT 42", -1, &stmt, NULL) == 0);
  CHECK(sqlite3_close(db) == 5);
  CHECK(sqlite3_close_v2(db) == 0);
  db = NULL;
  CHECK(sqlite3_step(stmt) == 100 && sqlite3_column_int(stmt, 0) == 42);
  CHECK(sqlite3_step(stmt) == 101);
cleanup:
  sqlite3_finalize(stmt);
  if (db) sqlite3_close(db);
  return result;
}

// SQLite's UTF-16 interfaces use native byte order and byte lengths, including
// embedded NULs with explicit lengths. Returned names survive until finalize.
// https://www.sqlite.org/c3ref/column_name.html
// https://www.sqlite.org/c3ref/column_blob.html
// https://www.sqlite.org/c3ref/bind_blob.html
int test_sqlite_utf16_names_bindings_and_values(void) {
  sqlite3 *db = NULL;
  sqlite3_stmt *stmt = NULL;
  int result = 0;
  if (!resolve_sqlite()) return -__LINE__;
  const void *(*name16)(sqlite3_stmt *, int) = dlsym((void *)-2, "sqlite3_column_name16");
  const void *(*text16)(sqlite3_stmt *, int) = dlsym((void *)-2, "sqlite3_column_text16");
  int (*bytes16)(sqlite3_stmt *, int) = dlsym((void *)-2, "sqlite3_column_bytes16");
  int (*bind16)(sqlite3_stmt *, int, const void *, int, void *) = dlsym((void *)-2, "sqlite3_bind_text16");
  int (*parameter_index)(sqlite3_stmt *, const char *) = dlsym((void *)-2, "sqlite3_bind_parameter_index");
  const void *(*errmsg16)(sqlite3 *) = dlsym((void *)-2, "sqlite3_errmsg16");
  CHECK(name16 && text16 && bytes16 && bind16 && parameter_index && errmsg16);
  CHECK(sqlite3_open(":memory:", &db) == 0);
  CHECK(sqlite3_prepare_v2(db, "SELECT :value AS \"caf\xc3\xa9\", NULL, 12345", -1, &stmt, NULL) == 0);
  const uint16_t expected_name[] = {'c', 'a', 'f', 0xe9, 0};
  const uint16_t value[] = {0x00c5, 0, 0xd83d, 0xde3a};
  CHECK(memcmp(name16(stmt, 0), expected_name, sizeof(expected_name)) == 0);
  CHECK(parameter_index(stmt, ":value") == 1);
  CHECK(parameter_index(stmt, ":absent") == 0);
  CHECK(bind16(stmt, 1, value, sizeof(value), (void *)-1) == 0);
  CHECK(sqlite3_step(stmt) == 100);
  CHECK(bytes16(stmt, 0) == sizeof(value));
  CHECK(memcmp(text16(stmt, 0), value, sizeof(value)) == 0);
  CHECK(((const uint16_t *)text16(stmt, 0))[4] == 0);
  CHECK(text16(stmt, 1) == NULL && bytes16(stmt, 1) == 0);
  CHECK(sqlite3_column_bytes(stmt, 2) == 5 && bytes16(stmt, 2) == 10);
  CHECK(sqlite3_reset(stmt) == 0);
  CHECK(bind16(stmt, 1, value, -1, (void *)-1) == 0);
  CHECK(sqlite3_step(stmt) == 100 && bytes16(stmt, 0) == 2);
  CHECK(sqlite3_reset(stmt) == 0);
  CHECK(bind16(stmt, 1, NULL, 100, (void *)-1) == 0);
  CHECK(sqlite3_step(stmt) == 100 && sqlite3_column_type(stmt, 0) == 5);
  CHECK(sqlite3_exec(db, "invalid SQL", NULL, NULL, NULL) == 1);
  CHECK(((const uint16_t *)errmsg16(db))[0] != 0);
cleanup:
  sqlite3_finalize(stmt);
  if (db) sqlite3_close(db);
  return result;
}
