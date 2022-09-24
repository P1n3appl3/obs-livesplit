mod config;

use std::{
    borrow::Cow,
    fs::File,
    io::BufWriter,
    path::{Path, PathBuf},
};

use livesplit_core::{
    auto_splitting::Runtime,
    layout::LayoutState,
    rendering::software::Renderer,
    run::saver::livesplit::{save_timer, IoWrite},
    Layout, SharedTimer, Timer,
};
use log::*;
use obs_wrapper::{
    graphics::*, log::Logger, obs_register_module, obs_string, obs_sys, prelude::*, properties::*,
    source::*, wrapper::PtrWrapper,
};

use config::ConfigWatcher;

obs_register_module!(LiveSplitModule);
struct LiveSplitModule {
    context: ModuleContext,
}

struct Source {
    pub timer: SharedTimer,
    pub layout: Layout,
    pub autosplitter: Runtime,
    pub state: LayoutState,
    pub renderer: Renderer,
    pub image: GraphicsTexture,
    pub width: u32,
    pub height: u32,
    pub enable_hotkeys: bool,
    pub splits_watcher: ConfigWatcher,
    pub layout_watcher: ConfigWatcher,
    pub splitter_watcher: ConfigWatcher,
}

const SETTING_WIDTH: ObsString = obs_string!("width");
const SETTING_HEIGHT: ObsString = obs_string!("height");
const SETTING_SPLITS: ObsString = obs_string!("splits");
const SETTING_LAYOUT: ObsString = obs_string!("layout");
const SETTING_AUTOSPLITTER: ObsString = obs_string!("autosplitter");

impl Source {
    fn update_splits(&mut self, path: &Path) {
        if let Some(run) = config::parse_run(path) {
            if self.timer.write().unwrap().replace_run(run, true).is_ok() {
                info!("updated splits");
            } else {
                warn!("failed to reload splits")
            }
        } else {
            warn!("failed to read splits");
        }
    }

    fn update_autosplitter(&mut self, path: PathBuf) {
        self.autosplitter.unload_script_blocking().ok();
        if let Err(e) = self.autosplitter.load_script_blocking(path) {
            warn!("failed to load autosplitter: {e}");
        } else {
            info!("loaded autosplitter");
        }
    }

    fn update_layout(&mut self, path: &Path) {
        if let Some(layout) = config::parse_layout(path) {
            self.layout = layout;
            info!("updated layout");
        } else {
            warn!("failed to read layout");
        }
    }

    fn update_settings(&mut self, settings: &DataObj) {
        if let (Some(w), Some(h)) = (settings.get(SETTING_WIDTH), settings.get(SETTING_HEIGHT)) {
            if w != self.width || h != self.height {
                self.width = w;
                self.height = h;
                self.image = GraphicsTexture::new(w, h, GraphicsColorFormat::RGBA);
            }
        }
        if let Some(path) = settings.get::<Cow<str>>(SETTING_SPLITS) {
            let new = PathBuf::from(path.as_ref());
            if self.splits_watcher.path.as_ref() != Some(&new) {
                match self.splits_watcher.change_file(&new) {
                    Ok(_) => self.update_splits(&new),
                    Err(e) => warn!("failed to reload splits: {e}"),
                }
            }
        }
        if let Some(path) = settings.get::<Cow<str>>(SETTING_AUTOSPLITTER) {
            let new = PathBuf::from(path.as_ref());
            if self.splitter_watcher.path.as_ref() != Some(&new) {
                match self.layout_watcher.change_file(&new) {
                    Ok(_) => self.update_autosplitter(new),
                    Err(e) => warn!("failed to reload autosplitter: {e}"),
                }
            }
        }
        if let Some(path) = settings.get::<Cow<str>>(SETTING_LAYOUT) {
            let new = PathBuf::from(path.as_ref());
            if self.layout_watcher.path.as_ref() != Some(&new) {
                match self.layout_watcher.change_file(&new) {
                    Ok(_) => self.update_layout(&new),
                    Err(e) => warn!("failed to reload layout: {e}"),
                }
            }
        }
    }
}

impl Sourceable for Source {
    fn create(ctx: &mut CreatableSourceContext<Source>, _source: SourceContext) -> Source {
        let width = ctx.settings.get(SETTING_WIDTH).unwrap();
        let height = ctx.settings.get(SETTING_HEIGHT).unwrap();
        let timer = Timer::new(config::default_run()).unwrap().into_shared();
        let mut source = Self {
            timer: timer.clone(),
            layout: Layout::default_layout(),
            autosplitter: Runtime::new(timer),
            state: LayoutState::default(),
            renderer: Renderer::new(),
            image: GraphicsTexture::new(width, height, GraphicsColorFormat::RGBA),
            enable_hotkeys: true,
            width,
            height,
            splits_watcher: ConfigWatcher::default(),
            layout_watcher: ConfigWatcher::default(),
            splitter_watcher: ConfigWatcher::default(),
        };
        source.update_settings(&ctx.settings);
        hotkey!(ctx, "Split", split_or_start);
        hotkey!(ctx, "Reset", reset, true);
        hotkey!(ctx, "Undo", undo_split);
        hotkey!(ctx, "Skip", skip_split);
        hotkey!(ctx, "Pause", toggle_pause_or_start);
        hotkey!(ctx, "Undo All Pauses", undo_all_pauses);
        hotkey!(ctx, "Previous Comparison", switch_to_previous_comparison);
        hotkey!(ctx, "Next Comparison", switch_to_next_comparison);
        hotkey!(ctx, "Toggle Timing Method", toggle_timing_method);
        source
    }

    fn get_id() -> ObsString {
        obs_string!("livesplit")
    }

    fn get_type() -> SourceType {
        SourceType::INPUT
    }
}

unsafe extern "C" fn save_splits(
    _: *mut obs_sys::obs_properties_t,
    _: *mut obs_sys::obs_property_t,
    data: *mut std::ffi::c_void,
) -> bool {
    let source: &mut Source = &mut *data.cast();
    if let Some(path) = source.timer.read().unwrap().run().path() {
        if let Ok(file) = File::create(path) {
            if save_timer(&source.timer.read().unwrap(), IoWrite(BufWriter::new(file))).is_ok() {
                info!("saved splits");
            } else {
                warn!("failed to write splits to file");
            }
        }
    } else {
        warn!("failed to save splits: no splits file found")
    }
    // TODO: draining the splits_watcher event queue here doesn't work due to
    // debouncing. is it ok that we always load the splits immediately after
    // saving them to the file, or do we need to signal to the splits_watcher
    // that it should drop the next update?
    false
}

impl GetPropertiesSource for Source {
    fn get_properties(&mut self) -> Properties {
        let mut props = Properties::new();
        props.add(
            SETTING_WIDTH,
            obs_string!("Width"),
            NumberProp::new_int().with_range(100u16..1000),
        );
        props.add(
            SETTING_HEIGHT,
            obs_string!("Height"),
            NumberProp::new_int().with_range(100u16..1000),
        );
        props.add(
            SETTING_SPLITS,
            obs_string!("Splits"),
            PathProp::new(PathType::File).with_filter(obs_string!("Splits File (*.lss)")),
        );
        props.add(
            SETTING_LAYOUT,
            obs_string!("Layout"),
            PathProp::new(PathType::File)
                .with_filter(obs_string!("Livesplit Layout File (*.ls1l *.lsl)")),
        );
        props.add(
            SETTING_AUTOSPLITTER,
            obs_string!("Autosplitter"),
            PathProp::new(PathType::File).with_filter(obs_string!("WASM module (*.wasm)")),
        );
        // TODO: add wrapper for add_button, maybe pass state through with list of
        // callbacks like for hotkey? figure out how this is getting the data
        // pointer in the current callback
        unsafe {
            obs_sys::obs_properties_add_button(
                props.as_ptr_mut(),
                obs_string!("save_splits").as_ptr(),
                obs_string!("Save Splits").as_ptr(),
                Some(save_splits),
            );
        }

        props
    }
}

impl MouseWheelSource for Source {
    fn mouse_wheel(&mut self, _event: obs_sys::obs_mouse_event, _xdelta: i32, ydelta: i32) {
        use std::cmp::Ordering;
        match ydelta.cmp(&0) {
            Ordering::Less => self.layout.scroll_down(),
            Ordering::Equal => {}
            Ordering::Greater => self.layout.scroll_up(),
        }
    }
}

impl GetDefaultsSource for Source {
    fn get_defaults(settings: &mut DataObj) {
        settings.set_default::<i64>(SETTING_WIDTH, 350);
        settings.set_default::<i64>(SETTING_HEIGHT, 700);
    }
}

impl UpdateSource for Source {
    fn update(&mut self, settings: &mut DataObj, _context: &mut GlobalContext) {
        self.update_settings(settings);
    }
}

impl GetNameSource for Source {
    fn get_name() -> ObsString {
        obs_string!("LiveSplit")
    }
}

impl GetWidthSource for Source {
    fn get_width(&mut self) -> u32 {
        self.width
    }
}

impl GetHeightSource for Source {
    fn get_height(&mut self) -> u32 {
        self.height
    }
}

impl ActivateSource for Source {
    fn activate(&mut self) {
        self.enable_hotkeys = true;
    }
}

impl DeactivateSource for Source {
    fn deactivate(&mut self) {
        self.enable_hotkeys = false;
    }
}

impl VideoRenderSource for Source {
    fn video_render(&mut self, _ctx: &mut GlobalContext, _vid_ctx: &mut VideoRenderContext) {
        while let Some(p) = self.splits_watcher.check_events() {
            self.update_splits(&p)
        }
        while let Some(p) = self.splitter_watcher.check_events() {
            self.update_autosplitter(p)
        }
        while let Some(p) = self.layout_watcher.check_events() {
            self.update_layout(&p)
        }

        self.layout
            .update_state(&mut self.state, &self.timer.read().unwrap().snapshot());
        self.renderer.render(&self.state, [self.width, self.height]);
        self.image.set_image(
            self.renderer.image_data(),
            self.width * 4, // line size in bytes
            false,
        );
        self.image.draw(0, 0, self.width, self.height, false);
    }
}

impl Module for LiveSplitModule {
    fn new(context: ModuleContext) -> Self {
        Self { context }
    }

    fn get_ctx(&self) -> &ModuleContext {
        &self.context
    }

    fn load(&mut self, load_context: &mut LoadContext) -> bool {
        let source_info = load_context
            .create_source_builder::<Source>()
            .enable_get_name()
            .enable_get_width()
            .enable_get_height()
            .enable_get_properties()
            .enable_update()
            .enable_video_render()
            .enable_mouse_wheel()
            .with_icon(Icon::GameCapture)
            .enable_get_defaults()
            .enable_activate()
            .enable_deactivate()
            .build();
        load_context.register_source(source_info);
        Logger::new().init().is_ok()
    }

    fn description() -> ObsString {
        obs_string!("A speedrun timer")
    }

    fn name() -> ObsString {
        obs_string!("LiveSplit One")
    }

    fn author() -> ObsString {
        obs_string!("Pineapple")
    }
}
