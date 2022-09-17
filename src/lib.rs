mod config;

use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    time::Duration,
};

use livesplit_core::{
    auto_splitting::Runtime, layout::LayoutState, rendering::software::Renderer, Layout,
    SharedTimer, Timer,
};
use log::{info, warn};
use notify::DebouncedEvent;
use obs_wrapper::{
    graphics::*, log::Logger, obs_register_module, obs_string, prelude::*, properties::*, source::*,
};
// use obs_wrapper::{obs_sys, wrapper::PtrWrapper};

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
        if let Some(path) = settings.get::<Cow<str>, _>(SETTING_SPLITS) {
            let new = PathBuf::from(path.as_ref());
            if self.splits_watcher.path.as_ref() != Some(&new) {
                self.splits_watcher.change_file(&new).unwrap();
                self.update_splits(&new);
            }
        }
        if let Some(path) = settings.get::<Cow<str>, _>(SETTING_AUTOSPLITTER) {
            let new = PathBuf::from(path.as_ref());
            if self.splitter_watcher.path.as_ref() != Some(&new) {
                self.splitter_watcher.change_file(&new).unwrap();
                self.update_autosplitter(new);
            }
        }
        if let Some(path) = settings.get::<Cow<str>, _>(SETTING_LAYOUT) {
            let new = PathBuf::from(path.as_ref());
            if self.layout_watcher.path.as_ref() != Some(&new) {
                self.layout_watcher.change_file(&new).unwrap();
                self.update_layout(&new);
            }
        }
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        info!("source destroyed")
    }
}

impl Sourceable for Source {
    fn create(ctx: &mut CreatableSourceContext<Source>, _source: SourceContext) -> Source {
        let (width, height) = (300, 500);
        let timer = Timer::new(config::default_run()).unwrap().into_shared();
        let mut state = Self {
            timer: timer.clone(),
            layout: Layout::default_layout(),
            autosplitter: Runtime::new(timer),
            state: LayoutState::default(),
            renderer: Renderer::new(),
            image: GraphicsTexture::new(width, height, GraphicsColorFormat::RGBA),
            width,
            height,
            splits_watcher: ConfigWatcher::new(Duration::from_millis(200)),
            layout_watcher: ConfigWatcher::new(Duration::from_millis(200)),
            splitter_watcher: ConfigWatcher::new(Duration::from_millis(200)),
        };
        state.update_settings(&ctx.settings);
        info!("created livesplit source");
        state
    }

    fn get_id() -> ObsString {
        obs_string!("livesplit")
    }

    fn get_type() -> SourceType {
        SourceType::INPUT
    }
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
            SETTING_LAYOUT,
            obs_string!("Layout File (.ls1l)"),
            PathProp::new(PathType::File),
        );
        props.add(
            SETTING_SPLITS,
            obs_string!("Splits File (.lss)"),
            PathProp::new(PathType::File),
        );
        props.add(
            SETTING_AUTOSPLITTER,
            obs_string!("Autosplitter module (.wasm)"),
            PathProp::new(PathType::File),
        );
        // TODO: add wrapper for add_button (how to pass state through?)
        // unsafe {
        //     obs_sys::obs_properties_add_button(
        //         props.as_ptr() as *mut obs_sys::obs_properties_t,
        //         obs_string!("save_splits").as_ptr(),
        //         obs_string!("Save Splits").as_ptr(),
        //         Some(callback),
        //     );
        // }
        // TODO: drain splits watcher upon saving splits?

        props
    }
}

// TODO: https://github.com/bennetthardwick/rust-obs-plugins/pull/15
impl GetDefaultsSource for Source {
    fn get_defaults(_settings: &mut DataObj) {
        unimplemented!()
    }
}

impl UpdateSource for Source {
    fn update(&mut self, settings: &mut DataObj, _context: &mut GlobalContext) {
        info!("settings update");
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

impl VideoRenderSource for Source {
    fn video_render(&mut self, _ctx: &mut GlobalContext, _vid_ctx: &mut VideoRenderContext) {
        use DebouncedEvent::*;
        while let Ok(event) = self.splits_watcher.rx.try_recv() {
            if let Create(p) | Write(p) = event {
                self.update_splits(&p)
            }
        }
        while let Ok(event) = self.splitter_watcher.rx.try_recv() {
            if let Create(p) | Write(p) = event {
                self.update_autosplitter(p)
            }
        }
        while let Ok(event) = self.layout_watcher.rx.try_recv() {
            if let Create(p) | Write(p) = event {
                self.update_layout(&p)
            }
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
        let source = load_context
            .create_source_builder::<Source>()
            .enable_get_name()
            .enable_get_width()
            .enable_get_height()
            .enable_get_properties()
            .enable_update()
            .enable_video_render()
            // .enable_get_defaults()
            // .enable_activate()
            // .enable_deactivate()
            .build();
        // TODO: set source icon_type
        // TODO: deactivate hotkeys on swap (just set a flag in activate/deactivate)
        // TODO: add "interactive" (scroll)

        load_context.register_source(source);
        Logger::new().init().is_ok()
    }

    fn description() -> ObsString {
        obs_string!("A speedrun timer")
    }

    fn name() -> ObsString {
        obs_string!("LiveSplit")
    }

    fn author() -> ObsString {
        obs_string!("Pineapple")
    }
}
