//! Exercise actual callers while another thread repeatedly swaps a parent.
use std::{
    fs,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};
#[tokio::test]
async fn tools_and_includes_never_return_replacement_directory_contents() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/prompt.md"), "approved source").unwrap();
    fs::write(outside.path().join("prompt.md"), "OUTSIDE_PRIVATE_CONTENT").unwrap();
    let mut tools =
        crate::tools::Tools::new(root.path().to_path_buf(), Default::default()).unwrap();
    let action =
        crate::tools::parse_action("action: tool\ntool: read\npath: src/prompt.md").unwrap();
    assert_eq!(tools.call(&action).await.unwrap(), "approved source");
    let stop = Arc::new(AtomicBool::new(false));
    let cycles = Arc::new(AtomicUsize::new(0));
    let (worker_stop, worker_cycles) = (stop.clone(), cycles.clone());
    let (project, host) = (root.path().to_path_buf(), outside.path().to_path_buf());
    let swap = std::thread::spawn(move || {
        while !worker_stop.load(Ordering::SeqCst) {
            fs::rename(project.join("src"), project.join("parked")).unwrap();
            std::os::unix::fs::symlink(&host, project.join("src")).unwrap();
            std::thread::yield_now();
            fs::remove_file(project.join("src")).unwrap();
            fs::rename(project.join("parked"), project.join("src")).unwrap();
            worker_cycles.fetch_add(1, Ordering::SeqCst);
        }
    });
    let mut returned = Vec::new();
    for _ in 0..500 {
        returned.push(tools.call(&action).await);
        returned.push(crate::config::render_includes(
            "{{ include \"src/prompt.md\" }}",
            root.path(),
            root.path(),
        ));
        std::thread::yield_now();
    }
    stop.store(true, Ordering::SeqCst);
    swap.join().unwrap();
    assert!(cycles.load(Ordering::SeqCst) > 0);
    for value in returned.into_iter().flatten() {
        assert_eq!(value, "approved source");
    }
    assert_eq!(
        fs::read_to_string(outside.path().join("prompt.md")).unwrap(),
        "OUTSIDE_PRIVATE_CONTENT"
    );
}
