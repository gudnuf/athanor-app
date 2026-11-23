use athanor_core::Store;

fn tmp() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ffi-export-{}", athanor_core::ids::new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn store_only_export_round_trips_without_an_engine() {
    let dir = tmp();
    let src = dir.join("live.sqlite");
    let tid = {
        let store = Store::open(&src, "device").unwrap();
        store
            .open_thread("what is entropy?", None, None)
            .unwrap()
            .id
    }; // dropped: file is closed and migrated
    let doc = ffi::export::export_vault_at(src.to_string_lossy().into()).unwrap();
    assert!(doc.starts_with("---\nathanor_vault: 1\n"));
    assert!(doc.contains(&tid));
    let dest = dir.join("copy.sqlite");
    ffi::export::export_database_copy_at(
        src.to_string_lossy().into(),
        dest.to_string_lossy().into(),
    )
    .unwrap();
    assert!(Store::open_read_only(&dest, "device")
        .unwrap()
        .get_thread(&tid)
        .is_ok());
    // refuses overwrite; refuses a missing source without creating it
    assert!(ffi::export::export_database_copy_at(
        src.to_string_lossy().into(),
        dest.to_string_lossy().into()
    )
    .is_err());
    let missing = dir.join("missing.sqlite");
    assert!(ffi::export::export_vault_at(missing.to_string_lossy().into()).is_err());
    assert!(!missing.exists());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn export_works_while_another_connection_holds_the_store() {
    let dir = tmp();
    let src = dir.join("live.sqlite");
    let live = Store::open(&src, "device").unwrap(); // kept alive, as the real engine would
    let tid = live.open_thread("held open?", None, None).unwrap().id;
    let doc = ffi::export::export_vault_at(src.to_string_lossy().into()).unwrap();
    assert!(doc.contains(&tid));
    drop(live);
    std::fs::remove_dir_all(dir).ok();
}
