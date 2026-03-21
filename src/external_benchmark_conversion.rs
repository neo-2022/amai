use anyhow::{Context, Result, anyhow};
use arrow_array::builder::{Float32Builder, Int32Builder, ListBuilder};
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use hdf5::File as Hdf5File;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const CONVERSION_BATCH_ROWS: usize = 2048;
const VDBBENCH_CUSTOM_DATASET_NAME: &str = "AmaiExternal";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorDbBenchBundle {
    pub dataset_root: PathBuf,
    pub dataset_name: String,
    pub dataset_dir: String,
    pub bundle_dir: PathBuf,
    pub train_file_count: usize,
    pub train_rows: usize,
    pub test_rows: usize,
    pub neighbors_rows: usize,
    pub dim: usize,
    pub metric_type: String,
    pub manifest_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct VectorDbBenchBundleManifest {
    generated_at_epoch_s: u64,
    source_dataset_path: String,
    dataset_code: String,
    dataset_display_name: String,
    distance: String,
    metric_type: String,
    dimensions: usize,
    train_rows: usize,
    test_rows: usize,
    neighbors_rows: usize,
    train_file_count: usize,
}

pub fn ensure_vectordbbench_bundle(
    repo_root: &Path,
    dataset_code: &str,
    dataset_display_name: &str,
    dataset_path: &Path,
    distance: &str,
    dimensions: usize,
) -> Result<VectorDbBenchBundle> {
    let dataset_root = repo_root
        .join("state")
        .join("external-benchmarks")
        .join("converted")
        .join("vectordbbench");
    let dataset_dir = dataset_code.to_string();
    let bundle_dir = dataset_root
        .join(VDBBENCH_CUSTOM_DATASET_NAME.to_ascii_lowercase())
        .join(&dataset_dir);
    fs::create_dir_all(&bundle_dir)
        .with_context(|| format!("failed to create {}", bundle_dir.display()))?;

    let manifest_path = bundle_dir.join("conversion_manifest.json");
    let train_path = bundle_dir.join("train.parquet");
    let test_path = bundle_dir.join("test.parquet");
    let neighbors_path = bundle_dir.join("neighbors.parquet");

    if manifest_path.exists()
        && train_path.exists()
        && test_path.exists()
        && neighbors_path.exists()
    {
        let raw = fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let manifest: VectorDbBenchBundleManifest = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
        if manifest.source_dataset_path == dataset_path.display().to_string()
            && manifest.dataset_code == dataset_code
            && manifest.dimensions == dimensions
        {
            return Ok(VectorDbBenchBundle {
                dataset_root,
                dataset_name: VDBBENCH_CUSTOM_DATASET_NAME.to_string(),
                dataset_dir,
                bundle_dir,
                train_file_count: manifest.train_file_count,
                train_rows: manifest.train_rows,
                test_rows: manifest.test_rows,
                neighbors_rows: manifest.neighbors_rows,
                dim: manifest.dimensions,
                metric_type: manifest.metric_type,
                manifest_path,
            });
        }
    }

    let hdf5 = Hdf5File::open(dataset_path)
        .with_context(|| format!("failed to open HDF5 dataset {}", dataset_path.display()))?;
    let train_ds = hdf5
        .dataset("train")
        .context("missing train dataset in HDF5")?;
    let test_ds = hdf5
        .dataset("test")
        .context("missing test dataset in HDF5")?;
    let neighbors_ds = hdf5
        .dataset("neighbors")
        .context("missing neighbors dataset in HDF5")?;

    let train_shape = train_ds.shape();
    let test_shape = test_ds.shape();
    let neighbors_shape = neighbors_ds.shape();
    if train_shape.len() != 2 || test_shape.len() != 2 || neighbors_shape.len() != 2 {
        return Err(anyhow!("expected 2D train/test/neighbors datasets in HDF5"));
    }
    if train_shape[1] != dimensions || test_shape[1] != dimensions {
        return Err(anyhow!(
            "HDF5 dim mismatch: expected {}, train={}, test={}",
            dimensions,
            train_shape[1],
            test_shape[1]
        ));
    }
    if neighbors_shape[0] != test_shape[0] {
        return Err(anyhow!(
            "neighbors rows {} do not match test rows {}",
            neighbors_shape[0],
            test_shape[0]
        ));
    }

    let train_rows = train_shape[0];
    let test_rows = test_shape[0];
    let dim = train_shape[1];
    let neighbor_width = neighbors_shape[1];

    let train_values = train_ds
        .read_raw::<f32>()
        .context("failed to read train values from HDF5")?;
    let test_values = test_ds
        .read_raw::<f32>()
        .context("failed to read test values from HDF5")?;
    let neighbor_values = neighbors_ds
        .read_raw::<i64>()
        .context("failed to read neighbors values from HDF5")?;

    write_vector_parquet(&train_path, &train_values, train_rows, dim)
        .with_context(|| format!("failed to write {}", train_path.display()))?;
    write_vector_parquet(&test_path, &test_values, test_rows, dim)
        .with_context(|| format!("failed to write {}", test_path.display()))?;
    write_neighbors_parquet(&neighbors_path, &neighbor_values, test_rows, neighbor_width)
        .with_context(|| format!("failed to write {}", neighbors_path.display()))?;

    let manifest = VectorDbBenchBundleManifest {
        generated_at_epoch_s: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        source_dataset_path: dataset_path.display().to_string(),
        dataset_code: dataset_code.to_string(),
        dataset_display_name: dataset_display_name.to_string(),
        distance: distance.to_string(),
        metric_type: map_distance_to_vdbbench_metric(distance)?.to_string(),
        dimensions: dim,
        train_rows,
        test_rows,
        neighbors_rows: test_rows,
        train_file_count: 1,
    };
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;

    Ok(VectorDbBenchBundle {
        dataset_root,
        dataset_name: VDBBENCH_CUSTOM_DATASET_NAME.to_string(),
        dataset_dir,
        bundle_dir,
        train_file_count: manifest.train_file_count,
        train_rows: manifest.train_rows,
        test_rows: manifest.test_rows,
        neighbors_rows: manifest.neighbors_rows,
        dim: manifest.dimensions,
        metric_type: manifest.metric_type,
        manifest_path,
    })
}

pub fn map_distance_to_vdbbench_metric(distance: &str) -> Result<&'static str> {
    match distance.to_ascii_lowercase().as_str() {
        "cosine" | "angular" => Ok("COSINE"),
        "euclidean" | "l2" => Ok("L2"),
        "dot" | "ip" | "innerproduct" => Ok("IP"),
        other => Err(anyhow!(
            "unsupported VectorDBBench metric mapping for distance {}",
            other
        )),
    }
}

fn write_vector_parquet(path: &Path, values: &[f32], rows: usize, dim: usize) -> Result<()> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int32, false),
        Field::new(
            "emb",
            DataType::List(Arc::new(Field::new("item", DataType::Float32, true))),
            false,
        ),
    ]));
    let writer_props = WriterProperties::builder().build();
    let file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    let mut writer = ArrowWriter::try_new(file, schema.clone(), Some(writer_props))
        .with_context(|| format!("failed to open parquet writer {}", path.display()))?;

    for start in (0..rows).step_by(CONVERSION_BATCH_ROWS) {
        let end = rows.min(start + CONVERSION_BATCH_ROWS);
        let batch_rows = end - start;
        let slice = &values[start * dim..end * dim];
        let batch = build_vector_batch(schema.clone(), slice, batch_rows, dim, start)?;
        writer
            .write(&batch)
            .with_context(|| format!("failed to append batch into {}", path.display()))?;
    }
    writer.close().context("failed to close parquet writer")?;
    Ok(())
}

fn write_neighbors_parquet(path: &Path, values: &[i64], rows: usize, width: usize) -> Result<()> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int32, false),
        Field::new(
            "neighbors_id",
            DataType::List(Arc::new(Field::new("item", DataType::Int32, true))),
            false,
        ),
    ]));
    let writer_props = WriterProperties::builder().build();
    let file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    let mut writer = ArrowWriter::try_new(file, schema.clone(), Some(writer_props))
        .with_context(|| format!("failed to open parquet writer {}", path.display()))?;

    for start in (0..rows).step_by(CONVERSION_BATCH_ROWS) {
        let end = rows.min(start + CONVERSION_BATCH_ROWS);
        let batch_rows = end - start;
        let slice = &values[start * width..end * width];
        let batch = build_neighbors_batch(schema.clone(), slice, batch_rows, width, start)?;
        writer
            .write(&batch)
            .with_context(|| format!("failed to append batch into {}", path.display()))?;
    }
    writer.close().context("failed to close parquet writer")?;
    Ok(())
}

fn build_vector_batch(
    schema: Arc<Schema>,
    values: &[f32],
    rows: usize,
    dim: usize,
    start_id: usize,
) -> Result<RecordBatch> {
    let mut id_builder = Int32Builder::with_capacity(rows);
    let mut emb_builder = ListBuilder::new(Float32Builder::with_capacity(rows * dim));
    for row in 0..rows {
        id_builder.append_value((start_id + row) as i32);
        emb_builder
            .values()
            .append_slice(&values[row * dim..(row + 1) * dim]);
        emb_builder.append(true);
    }
    let columns: Vec<ArrayRef> = vec![
        Arc::new(id_builder.finish()),
        Arc::new(emb_builder.finish()),
    ];
    RecordBatch::try_new(schema, columns).context("failed to build vector record batch")
}

fn build_neighbors_batch(
    schema: Arc<Schema>,
    values: &[i64],
    rows: usize,
    width: usize,
    start_id: usize,
) -> Result<RecordBatch> {
    let mut id_builder = Int32Builder::with_capacity(rows);
    let mut neighbors_builder = ListBuilder::new(Int32Builder::with_capacity(rows * width));
    for row in 0..rows {
        id_builder.append_value((start_id + row) as i32);
        for value in &values[row * width..(row + 1) * width] {
            let converted = i32::try_from(*value)
                .with_context(|| format!("neighbor id {} does not fit in int32", value))?;
            neighbors_builder.values().append_value(converted);
        }
        neighbors_builder.append(true);
    }
    let columns: Vec<ArrayRef> = vec![
        Arc::new(id_builder.finish()),
        Arc::new(neighbors_builder.finish()),
    ];
    RecordBatch::try_new(schema, columns).context("failed to build neighbors record batch")
}

#[cfg(test)]
mod tests {
    use super::map_distance_to_vdbbench_metric;

    #[test]
    fn maps_distances_to_vdbbench_metric_types() {
        assert_eq!(
            map_distance_to_vdbbench_metric("cosine").expect("metric"),
            "COSINE"
        );
        assert_eq!(
            map_distance_to_vdbbench_metric("euclidean").expect("metric"),
            "L2"
        );
        assert_eq!(
            map_distance_to_vdbbench_metric("dot").expect("metric"),
            "IP"
        );
    }
}
