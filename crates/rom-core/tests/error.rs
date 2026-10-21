use rom_core::error::{Error, Result};

#[test]
fn io_error_converts_into_core_error_via_question_mark() {
    fn fails() -> Result<()> {
        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"))?;
        Ok(())
    }

    let err = fails().unwrap_err();
    assert!(matches!(err, Error::Io(_)));
    assert_eq!(err.to_string(), "I/O error: missing");
}
