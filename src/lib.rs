mod utils;

use anyhow::Result;
use candle::{IndexOp, Tensor};
use candle_transformers::models::mimi;
use eframe::egui;
use std::sync::{Arc, Mutex};

const BRIA_CODES: &[u8] = include_bytes!("../assets/bria.safetensors");

pub struct PcmOutBuffer {
    pcm: Arc<Mutex<Vec<f32>>>,
}

impl oboe::AudioOutputCallback for PcmOutBuffer {
    type FrameType = (f32, oboe::Mono);

    fn on_audio_ready(
        &mut self,
        _stream: &mut dyn oboe::AudioOutputStreamSafe,
        frames: &mut [f32],
    ) -> oboe::DataCallbackResult {
        let mut pcm = self.pcm.lock().unwrap();

        if frames.len() <= pcm.len() {
            frames.fill(0f32);
            frames.copy_from_slice(&pcm[..frames.len()]);
            pcm.drain(0..frames.len());
        }
        oboe::DataCallbackResult::Continue
    }
}

pub struct PcmInBuffer(std::sync::mpsc::Sender<Vec<f32>>);
impl oboe::AudioInputCallback for PcmInBuffer {
    type FrameType = (f32, oboe::Mono);

    fn on_audio_ready(
        &mut self,
        _stream: &mut dyn oboe::AudioInputStreamSafe,
        frames: &[f32],
    ) -> oboe::DataCallbackResult {
        if let Err(err) = self.0.send(frames.to_vec()) {
            log::error!("record err {err:?}");
            oboe::DataCallbackResult::Stop
        } else {
            oboe::DataCallbackResult::Continue
        }
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use android_logger::Config;
    android_logger::init_once(Config::default().with_max_level(log::LevelFilter::Info));

    #[cfg(feature = "executorch")]
    xctch::et_pal_init();

    log::info!("requesting permissions");
    if let Err(err) = req_perm(&app) {
        log::error!("error requesting perm: {err:?}")
    }

    // Use a single thread for mimi, using the default of 8 threads made things a lot worse on
    // a pixel 6a.
    std::env::set_var("RAYON_NUM_THREADS", "1");

    let options = eframe::NativeOptions { android_app: Some(app), ..Default::default() };
    let model = make_mimi();
    if let Err(err) = model.as_ref() {
        log::error!("retrieving weights failed {err:?}")
    }
    eframe::run_native(
        "moshi demo",
        options,
        Box::new(|cc| {
            // This gives us image support:
            egui_extras::install_image_loaders(&cc.egui_ctx);
            cc.egui_ctx.set_zoom_factor(2.0);
            Ok(Box::new(MyApp {
                play_stream: None,
                record_stream: None,
                status: "ok".to_string(),
                model,
            }))
        }),
    )
    .unwrap()
}

struct MyApp {
    status: String,
    play_stream: Option<oboe::AudioStreamAsync<oboe::Output, PcmOutBuffer>>,
    record_stream: Option<oboe::AudioStreamAsync<oboe::Input, PcmInBuffer>>,
    model: Result<mimi::encodec::Encodec>,
}

fn model_encode(
    mut model: mimi::encodec::Encodec,
    rx: std::sync::mpsc::Receiver<Vec<f32>>,
) -> Result<()> {
    loop {
        let pcm = rx.recv()?;
        let start = std::time::Instant::now();
        let pcm_len = pcm.len();
        let pcm = Tensor::from_vec(pcm, (1, 1, pcm_len), &candle::Device::Cpu)?;
        let codes = model.encode_step(&pcm.into())?;
        log::info!("encode step {:?} in {:?}", codes.shape(), start.elapsed());
    }
}

fn model_decode(mut model: mimi::encodec::Encodec, pcm_b: Arc<Mutex<Vec<f32>>>) -> Result<()> {
    log::info!("running model on {} threads", candle::utils::get_num_threads());
    let codes = candle::safetensors::load_buffer(BRIA_CODES, &candle::Device::Cpu)?;
    let codes = match codes.get("codes") {
        Some(tensor) => tensor.clone(),
        None => anyhow::bail!("cannot find codes"),
    };
    let len = codes.dim(candle::D::Minus1)?;
    for idx in 0..len {
        let start = std::time::Instant::now();
        let codes = codes.narrow(candle::D::Minus1, idx, 1)?;
        let pcm = model.decode_step(&codes.into())?;
        if let Some(pcm) = pcm.as_option() {
            let pcm = pcm.i(0)?.i(0)?;
            let mut pcm = pcm.to_vec1::<f32>()?;
            pcm_b.lock().unwrap().append(&mut pcm);
        }
        log::info!("decode step in {:?}", start.elapsed());
        let count = Arc::strong_count(&pcm_b);
        if count == 1 {
            log::info!("arc count is one, exiting thread");
            break;
        }
    }
    Ok(())
}

impl MyApp {
    fn start_play(&mut self) -> Result<()> {
        use oboe::AudioStream;

        self.status = "playing".to_string();
        self.play_stream = None;
        let pcm = Arc::new(Mutex::new(vec![]));
        if let Ok(model) = self.model.as_ref() {
            let model = model.clone();
            let pcm_b = pcm.clone();
            log::info!("launching model");
            std::thread::spawn(move || {
                if let Err(err) = model_decode(model, pcm_b) {
                    log::error!("decode err {err:?}")
                }
            });
        }
        let mut stream = oboe::AudioStreamBuilder::default()
            .set_performance_mode(oboe::PerformanceMode::LowLatency)
            .set_sharing_mode(oboe::SharingMode::Shared)
            .set_format::<f32>()
            .set_channel_count::<oboe::Mono>()
            .set_sample_rate(24_000)
            .set_frames_per_callback(1920)
            .set_callback(PcmOutBuffer { pcm: pcm.clone() })
            .open_stream()?;

        stream.start()?;
        self.play_stream = Some(stream);
        Ok(())
    }

    fn start_record(&mut self) -> Result<()> {
        use oboe::AudioStream;

        self.status = "recording".to_string();
        self.record_stream = None;
        let (tx, rx) = std::sync::mpsc::channel();
        if let Ok(model) = self.model.as_ref() {
            let model = model.clone();
            std::thread::spawn(move || {
                if let Err(err) = model_encode(model, rx) {
                    log::error!("encode err {err:?}")
                }
            });
        }
        let mut stream = oboe::AudioStreamBuilder::default()
            .set_input()
            .set_performance_mode(oboe::PerformanceMode::LowLatency)
            .set_sharing_mode(oboe::SharingMode::Shared)
            .set_format::<f32>()
            .set_channel_count::<oboe::Mono>()
            .set_sample_rate(24_000)
            .set_frames_per_callback(1920)
            .set_callback(PcmInBuffer(tx))
            .open_stream()?;
        stream.start()?;
        self.record_stream = Some(stream);
        Ok(())
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let with_neon = candle::utils::with_neon();
        std::thread::sleep(std::time::Duration::from_secs_f32(0.1));
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.);
            ui.heading("Kyutai - Moshi");
            ui.label(format!("status: {:?}", self.status));
            if ui.button("Play").clicked() {
                if let Err(err) = self.start_play() {
                    self.status = format!("err {err:?}")
                }
            }
            if ui.button("Record").clicked() {
                if let Err(err) = self.start_record() {
                    self.status = format!("err {err:?}")
                }
            }
            if ui.button("Stop").clicked() {
                self.status = "stopped".to_string();
                self.play_stream = None;
                self.record_stream = None;
            }
            ui.label(format!("neon {:?}", with_neon));
            ui.label(format!("build {:?}", crate::utils::BUILD_INFO.build_timestamp));

            ui.image(egui::include_image!("../assets/ferris.png"));
        });
    }
}

fn get_cache_dir() -> Result<std::path::PathBuf> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let ctx = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };
    let cache_dir = env.call_method(ctx, "getFilesDir", "()Ljava/io/File;", &[])?.l()?;
    let cache_dir: jni::objects::JString =
        env.call_method(&cache_dir, "toString", "()Ljava/lang/String;", &[])?.l()?.into();
    let cache_dir = env.get_string(&cache_dir)?;
    let cache_dir = cache_dir.to_str()?;
    Ok(std::path::PathBuf::from(cache_dir))
}

fn make_mimi() -> Result<mimi::encodec::Encodec> {
    log::info!("make mimi");
    let cache_dir = get_cache_dir()?;
    let api =
        hf_hub::api::sync::ApiBuilder::from_cache(hf_hub::Cache::new(cache_dir.to_path_buf()))
            .build()?;
    let model = api.model("kyutai/mimi".to_string());
    log::info!("retrieving weights");
    let model = model.get("model.safetensors")?;
    log::info!("retrieving weights done");
    let model = model.to_string_lossy();
    let mut model = mimi::load(&model, Some(8), &candle::Device::Cpu)?;
    log::info!("warming up the model");
    let fake_pcm = candle::Tensor::zeros((1, 1, 1920), candle::DType::F32, &candle::Device::Cpu)?;
    let codes = model.encode_step(&fake_pcm.into())?;
    let pcm = model.decode_step(&codes)?;
    log::info!("warmed up model {:?}", pcm.shape());
    model.reset_state();
    Ok(model)
}

fn req_perm(app: &winit::platform::android::activity::AndroidApp) -> Result<()> {
    use jni::objects::JObject;
    use jni::sys::jobject;

    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let record_string = env.new_string("android.permission.RECORD_AUDIO")?;
    let string_class = env.find_class("java/lang/String")?;
    let perms = env.new_object_array(1, string_class, record_string)?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as jobject) };
    let _ = env.call_method(
        activity,
        "requestPermissions",
        "([Ljava/lang/String;I)V",
        &[(&perms).into(), jni::objects::JValueGen::Int(1)],
    )?;
    Ok(())
}
