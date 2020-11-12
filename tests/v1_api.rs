//! Tests for the v1 API endpoints. These are integration style tests and, as such, they run through
//! entire user workflows

mod common;

#[tokio::test]
async fn test_successful_workflow() {
    // Upload the parcels for one of the invoices

    // Create an invoice pointing to those parcels and make sure the correct response is returned

    // Create a second version (pointing at the same parcel) of the same invoice

    // Create an invoice with missing parcels and make sure the correct response is returned
}

#[tokio::test]
async fn test_yank() {
    // Upload the parcels for one of the invoices

    // Yank the invoice

    // Attempt to fetch the invoice and make sure it doesn't return

    // Set yanked to true and attempt to fetch again
}

#[tokio::test]
// This isn't meant to test all of the possible validation failures (that should be done in a unit
// test for storage), just the main validation failures from the API
async fn test_invoice_validation() {
    // Already created invoice

    // Missing version
}

#[tokio::test]
// This isn't meant to test all of the possible validation failures (that should be done in a unit
// test for storage), just the main validation failures from the API
async fn test_parcel_validation() {
    // Already created parcel

    // Incorrect SHA

    // Missing size

    // Empty body?
}

#[tokio::test]
// Once again, this isn't meant to exercise all of the query functionality, just that the API
// functions properly
async fn test_queries() {
    // Insert data into store

    // Test empty query

    // Test query term filter

    // Test version queries

    // Test yank

    // Test limit/offset
}
