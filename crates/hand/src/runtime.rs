//! Open an ONNX model and run it on one input, on the CPU or the Qualcomm NPU.
//!
//! The NPU is a plug-in execution provider: its library is registered once per process
//! ([`Runtime::register_npu_library`]), after which the runtime lists its devices and a model can be
//! placed on the one that is an NPU. Which device a model really runs on is always reported, so a
//! result can say what produced it and a slow run can be explained.

use std::path::Path;
use std::sync::{Arc, Mutex};

use ort::environment::Environment;
use ort::memory::DeviceType;
use ort::session::Session;
use ort::value::Tensor;

use crate::error::{HandError, Result};

/// Anything that can run a network on one input. Tests use stand-ins for it.
pub trait Network {
    /// Run on one input of the given shape and return every output, flattened, in file order.
    ///
    /// # Errors
    ///
    /// Fails if the model cannot run on this input.
    fn run(&mut self, blob: Vec<f32>, shape: [usize; 4]) -> Result<Vec<Vec<f32>>>;
}

/// Where a model should run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    /// The NPU if there is one and it accepts the model, otherwise the CPU.
    Auto,
    /// Always the CPU.
    Cpu,
    /// The NPU, or fail.
    Npu,
}

/// Where a model actually runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// On the CPU.
    Cpu,
    /// On the NPU.
    Npu,
}

/// The ONNX Runtime library, loaded once per process.
#[derive(Clone)]
pub struct Runtime {
    environment: Arc<Environment>,
    /// Whether the NPU library has been registered (it may only be registered once).
    npu_registered: Arc<Mutex<bool>>,
}

impl Runtime {
    /// Load the runtime from the shared library at `library`, or from the path named by the
    /// `ORT_DYLIB_PATH` environment variable when `library` is `None`.
    ///
    /// Only one runtime may exist per process; create it once and clone it.
    ///
    /// # Errors
    ///
    /// Fails if the library cannot be found or loaded.
    pub fn load(library: Option<&Path>) -> Result<Self> {
        let builder = match library {
            Some(path) => ort::init_from(path).map_err(|e| HandError::Library(e.to_string()))?,
            None => ort::init(),
        };
        let environment = builder
            .with_name("axonhand")
            .build()
            .map_err(|e| HandError::Library(e.to_string()))?;
        Ok(Self {
            environment: Arc::new(environment),
            npu_registered: Arc::new(Mutex::new(false)),
        })
    }
}

impl Runtime {
    /// Register the NPU execution-provider library under `name` (for Qualcomm, the values of
    /// `onnxruntime_qnn.get_ep_name()` and `get_library_path()`). Calling it again does nothing.
    ///
    /// # Errors
    ///
    /// Fails if the library cannot be registered.
    pub fn register_npu_library(&self, name: &str, library: &Path) -> Result<()> {
        let mut done = self
            .npu_registered
            .lock()
            .map_err(|_| HandError::Library("lock poisoned".into()))?;
        if *done {
            return Ok(());
        }
        self.environment
            .register_ep_library(name, library)
            .map_err(|e| {
                HandError::Library(format!("the NPU library could not be registered: {e}"))
            })?;
        *done = true;
        Ok(())
    }

    /// Whether the runtime can see an NPU.
    #[must_use]
    pub fn has_npu(&self) -> bool {
        self.environment
            .devices()
            .any(|d| d.hardware_device().ty() == DeviceType::NPU)
    }
}

/// An open model.
pub struct Model {
    session: Session,
    placement: Placement,
    note: String,
}

impl Model {
    /// Load the model file at `path` on the CPU.
    ///
    /// # Errors
    ///
    /// Fails if the file is missing or the runtime cannot load it.
    pub fn open(runtime: &Runtime, path: &Path) -> Result<Self> {
        Self::open_on(runtime, path, Device::Cpu)
    }

    /// Load the model file at `path` on the requested device.
    ///
    /// `Device::Auto` falls back to the CPU and says why in [`Model::note`]; `Device::Npu` fails
    /// instead.
    ///
    /// # Errors
    ///
    /// Fails if the file is missing, the runtime cannot load it, or the NPU was demanded and is not
    /// available.
    pub fn open_on(runtime: &Runtime, path: &Path, device: Device) -> Result<Self> {
        if !path.is_file() {
            return Err(HandError::ModelMissing(path.to_path_buf()));
        }
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let mut note = String::new();
        if device != Device::Cpu {
            match Self::on_npu(runtime, path, &name) {
                Ok(session) => {
                    return Ok(Self {
                        session,
                        placement: Placement::Npu,
                        note,
                    })
                }
                Err(error) if device == Device::Npu => return Err(error),
                Err(error) => note = error.to_string(),
            }
        }
        let session = Session::builder(&runtime.environment)
            .and_then(|mut builder| builder.commit_from_file(path))
            .map_err(|e| HandError::Load {
                name,
                message: e.to_string(),
            })?;
        Ok(Self {
            session,
            placement: Placement::Cpu,
            note,
        })
    }

    fn on_npu(runtime: &Runtime, path: &Path, name: &str) -> Result<Session> {
        let devices: Vec<_> = runtime
            .environment
            .devices()
            .filter(|d| d.hardware_device().ty() == DeviceType::NPU)
            .collect();
        if devices.is_empty() {
            return Err(HandError::Load {
                name: name.into(),
                message: "no NPU was found".into(),
            });
        }
        let load = |e: ort::Error| HandError::Load {
            name: name.into(),
            message: format!("the NPU could not load it: {e}"),
        };
        let builder = Session::builder(&runtime.environment).map_err(load)?;
        let mut builder = builder
            .with_devices(devices.into_iter().take(1), None)
            .map_err(|e| load(e.into()))?;
        builder.commit_from_file(path).map_err(load)
    }

    /// Where the model runs.
    #[must_use]
    pub fn placement(&self) -> Placement {
        self.placement
    }

    /// Why the CPU is in use when the NPU was wanted; empty when nothing fell back.
    #[must_use]
    pub fn note(&self) -> &str {
        &self.note
    }
}

impl Network for Model {
    fn run(&mut self, blob: Vec<f32>, shape: [usize; 4]) -> Result<Vec<Vec<f32>>> {
        let dims: Vec<i64> = shape.iter().map(|&d| d as i64).collect();
        let input = Tensor::from_array((dims, blob)).map_err(|e| HandError::Run(e.to_string()))?;
        let outputs = self
            .session
            .run(ort::inputs![input])
            .map_err(|e| HandError::Run(e.to_string()))?;
        let mut result = Vec::with_capacity(outputs.len());
        for (_, value) in &outputs {
            let (_, data) = value
                .try_extract_tensor::<f32>()
                .map_err(|e| HandError::Output(e.to_string()))?;
            result.push(data.to_vec());
        }
        Ok(result)
    }
}
