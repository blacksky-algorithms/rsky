# Testing Guide for rsky-wintermute

This document describes the testing strategy and how to run tests for rsky-wintermute.

## Test Structure

The test suite consists of three main categories:

### 1. Storage Tests
Located in `src/storage.rs`, these tests verify the fjall-based storage layer:
- Firehose event serialization/deserialization
- Index queue operations
- Backfill queue operations
- Cursor management

### 2. Backfiller Tests
Located in `src/backfiller/tests.rs`, these tests verify CAR file processing and record conversion:
- IPLD record conversion (CID handling, nested structures)
- Real repository backfill processing with `did:plc:w4xbfzo7kqfes5zb7r6qv3rw`
- Validates ~7700 records are correctly enqueued

### 3. Indexer Tests
Located in `src/indexer/tests.rs`, these are comprehensive integration tests that verify end-to-end indexing:

#### `test_index_job_processing`
Full integration test that:
1. Backfills a real repository (`did:plc:w4xbfzo7kqfes5zb7r6qv3rw`)
2. Processes all ~7700 records through the indexer
3. Writes records to postgres database
4. Verifies counts for posts, likes, follows, reposts, and profiles
5. Cleans up test data

#### `test_notification_creation`
Tests that notifications are properly created for social interactions:
- Like notifications
- Follow notifications
- Repost notifications
- Starter pack join notifications

#### `test_uri_validation`
Tests AtUri validation:
- Invalid URIs are rejected
- Valid AT Protocol URIs are accepted
- URI parsing errors are handled gracefully

## Prerequisites

### Required Services

1. **PostgreSQL Database**
   - A test database with the full bsky schema
   - Default: `postgresql://postgres:postgres@localhost:5432/bsky_test`
   - Override with `DATABASE_URL` environment variable

2. **Internet Connection**
   - Required for backfiller tests (fetches real repos from PDS)

### Database Setup

The tests expect a PostgreSQL database with the complete Bluesky schema including:
- `post`, `like`, `follow`, `repost`, `block` tables
- `profile`, `feed_generator`, `list`, `list_item`, `list_block` tables
- `starter_pack`, `labeler`, `threadgate`, `postgate` tables
- `chat_declaration`, `notif_declaration`, `status`, `verification` tables
- `notification` table

## Running Tests

### Run All Tests
```bash
cargo test --package rsky-wintermute
```

### Run Only Unit Tests (No Database Required)
```bash
cargo test --package rsky-wintermute --lib -- \
    test_write_action_serialization \
    test_convert_record
```

### Run Indexer Integration Tests
```bash
# Requires postgres database
DATABASE_URL="postgresql://postgres:postgres@localhost:5432/bsky_test" \
cargo test --package rsky-wintermute --lib indexer::tests
```

### Run Specific Test
```bash
DATABASE_URL="postgresql://postgres:postgres@localhost:5432/bsky_test" \
cargo test --package rsky-wintermute --lib test_index_job_processing -- --nocapture
```

The `--nocapture` flag shows tracing output during test execution.

## Test Best Practices

### Isolation
- Each test uses a temporary fjall database via `TempDir`
- PostgreSQL data is cleaned up before and after each test
- Tests can run in parallel without conflicts

### Cleanup
The `cleanup_test_data` function removes all test data from postgres:
- Deletes by `creator`, `did`, or `author` fields
- Covers all 19 tables including notifications
- Called at start and end of tests

### Performance
- Backfill tests download ~7700 records (takes 5-10 seconds)
- Indexer tests process all records in batches (takes 10-20 seconds)
- Use `--test-threads=1` to run sequentially if needed

## Environment Variables

- `DATABASE_URL`: PostgreSQL connection string
  - Default: `postgresql://postgres:postgres@localhost:5432/bsky_test`
- `RUST_LOG`: Set to `info` or `debug` for verbose output
  - Example: `RUST_LOG=info cargo test -- --nocapture`

## Expected Test Results

When all tests pass, you should see:
```
running 13 tests
test storage::tests::test_cursor ... ok
test storage::tests::test_backfill_queue ... ok
test storage::tests::test_index_queue ... ok
test storage::tests::test_firehose_event_roundtrip ... ok
test indexer::tests::indexer_tests::test_write_action_serialization ... ok
test backfiller::tests::backfiller_tests::test_convert_record_preserves_objects ... ok
test backfiller::tests::backfiller_tests::test_convert_record_preserves_regular_arrays ... ok
test backfiller::tests::backfiller_tests::test_convert_record_converts_cid_bytes ... ok
test backfiller::tests::backfiller_tests::test_convert_record_handles_nested_structures ... ok
test indexer::tests::indexer_tests::test_uri_validation ... ok
test backfiller::tests::backfiller_tests::test_process_job_with_real_repo ... ok
test indexer::tests::indexer_tests::test_notification_creation ... ok
test indexer::tests::indexer_tests::test_index_job_processing ... ok

test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## Troubleshooting

### Database Connection Errors
- Ensure PostgreSQL is running
- Verify `DATABASE_URL` is correct
- Check database has required schema

### Backfill Timeout
- Increase timeout in test: `.timeout(std::time::Duration::from_secs(120))`
- Check internet connection
- Verify PDS is accessible

### Assertion Failures
- Check expected counts match actual data in repository
- Repository content may change over time
- Update assertions if repository grows significantly

## Coverage

The test suite provides:
- ✅ Unit tests for all data structures
- ✅ Integration tests for backfill → index → postgres flow
- ✅ Validation of AtUri parsing and construction
- ✅ Verification of notification creation logic
- ✅ Real-world data processing (7700+ records)
- ✅ All 18 AT Protocol collections handled
