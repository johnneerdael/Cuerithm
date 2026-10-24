use cuerithm_library::playback::CacheSource;
use cuerithm_library::waveform::{cached_waveform, waveform};
use cuerithm_media::MediaRuntime;
use rodio::Source;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
#[test]
fn every_format_shares_waveform_seek_and_analysis_clock() {
    let d = tempfile::tempdir().unwrap();
    let archive = std::fs::File::open(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/fixtures/media.tar.gz"),
    )
    .unwrap();
    tar::Archive::new(flate2::read::GzDecoder::new(archive))
        .unpack(d.path())
        .unwrap();
    let cases: serde_json::Value =
        serde_json::from_slice(&std::fs::read(d.path().join("cases.json")).unwrap()).unwrap();
    let root = std::env::var_os("CUERITHM_MEDIA_RUNTIME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/media-runtime/bin")
        });
    let rt = MediaRuntime::from_bundle(&root).unwrap();
    for case in cases.as_array().unwrap() {
        let audio = Arc::new(
            rt.decode(&d.path().join(case["file"].as_str().unwrap()), d.path())
                .unwrap(),
        );
        let peaks = waveform(&audio, 1024).unwrap();
        let cached = cached_waveform(&audio, 1024, &d.path().join("wave-cache")).unwrap();
        let reused = cached_waveform(&audio, 1024, &d.path().join("wave-cache")).unwrap();
        assert_eq!(
            serde_json::to_value(&peaks).unwrap(),
            serde_json::to_value(reused).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&peaks).unwrap(),
            serde_json::to_value(cached).unwrap()
        );
        // The largest resolution reuses analysed points instead of one FFT per bin.
        let fine = waveform(&audio, 16384).unwrap();
        assert_eq!(fine.bands.len(), 16384);
        assert_eq!(fine.levels.len(), 16384);
        assert!(fine.bands.iter().any(|b| b.contains(&255)));
        assert_eq!(peaks.bins.len(), 1024);
        assert_eq!(peaks.bands.len(), 1024);
        assert!(peaks.bands.iter().any(|b| b.contains(&255)));
        assert_eq!(peaks.rms.len(), 1024);
        assert_eq!(peaks.levels.len(), 1024);
        assert!(peaks.rms.iter().any(|r| *r > 0.));
        // Average level can never exceed the peak in the same bin.
        assert!(
            peaks
                .rms
                .iter()
                .zip(&peaks.bins)
                .all(|(r, b)| *r <= b.max.abs().max(b.min.abs()) + 1e-3)
        );
        assert!(
            peaks
                .levels
                .iter()
                .all(|l| l.iter().all(|v| v.is_finite() && *v >= 0.))
        );
        // A cache written before colour or levels existed must be rebuilt, not served.
        for missing in ["bands", "rms", "levels"] {
            for entry in std::fs::read_dir(d.path().join("wave-cache")).unwrap() {
                let path = entry.unwrap().path();
                let mut value: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                value.as_object_mut().unwrap().remove(missing);
                std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            let rebuilt = cached_waveform(&audio, 1024, &d.path().join("wave-cache")).unwrap();
            assert_eq!(
                serde_json::to_value(&peaks).unwrap(),
                serde_json::to_value(rebuilt).unwrap(),
                "cache without {missing} was served"
            );
        }
        assert_eq!(peaks.duration_seconds, audio.clock.duration_seconds());
        assert!(
            peaks
                .bins
                .iter()
                .all(|b| b.min.is_finite() && b.max.is_finite() && b.min <= b.max)
        );
        let frame = audio.frames / 2;
        let mut source = CacheSource::open(audio.clone()).unwrap();
        source
            .try_seek(Duration::from_secs_f64(frame as f64 / 44100.))
            .unwrap();
        assert_eq!(source.next(), Some(audio.read_frames(frame, 1).unwrap()[0]));
        assert!(waveform(&audio, 0).is_err());
    }
}

#[test]
fn a_waveform_read_from_the_cache_counts_as_used_so_trimming_keeps_it() {
    let d = tempfile::tempdir().unwrap();
    let archive = std::fs::File::open(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/fixtures/media.tar.gz"),
    )
    .unwrap();
    tar::Archive::new(flate2::read::GzDecoder::new(archive))
        .unpack(d.path())
        .unwrap();
    let cases: serde_json::Value =
        serde_json::from_slice(&std::fs::read(d.path().join("cases.json")).unwrap()).unwrap();
    let root = std::env::var_os("CUERITHM_MEDIA_RUNTIME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/media-runtime/bin")
        });
    let rt = MediaRuntime::from_bundle(&root).unwrap();
    let case = &cases.as_array().unwrap()[0];
    let audio = rt
        .decode(&d.path().join(case["file"].as_str().unwrap()), d.path())
        .unwrap();
    let cache = d.path().join("wave-cache");
    cached_waveform(&audio, 240, &cache).unwrap();
    let file = std::fs::read_dir(&cache)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let long_ago = std::time::SystemTime::now() - Duration::from_secs(30 * 24 * 3600);
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(long_ago)
        .unwrap();
    cached_waveform(&audio, 240, &cache).unwrap();
    let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
    assert!(
        modified > long_ago + Duration::from_secs(24 * 3600),
        "reading the cached waveform refreshes its time"
    );
}
