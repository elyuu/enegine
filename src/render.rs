use ash::extensions::{
    ext::DebugUtils,
    khr::{Surface, Swapchain},
};
use ash::version::{DeviceV1_0, EntryV1_0, InstanceV1_0};
use ash::vk;
use ash_window;

use std::borrow::Cow;
use std::ffi::CStr;

use winit::window;

pub struct Renderer {
    entry: ash::Entry,
    instance: ash::Instance,
    device: ash::Device,
    swapchain_loader: Swapchain,

    physical_device: vk::PhysicalDevice,
    queue_family_index: u32,
    present_queue: vk::Queue,

    surface_loader: Surface,
    surface: vk::SurfaceKHR,
    swapchain: vk::SwapchainKHR,

    // Debug
    pub debug_utils: Option<DebugUtils>,
    debug_messenger: vk::DebugUtilsMessengerEXT,
}

impl Renderer {
    // TODO: Don't really need window here, just required exts
    pub fn new(window: &window::Window) -> Result<Renderer, &'static str> {
        let entry = ash::Entry::new().unwrap();

        unsafe {
            // Instance
            let app_name = to_cstr!("Triangle");
            let app_info = vk::ApplicationInfo::builder()
                .application_name(app_name)
                .application_version(0)
                .engine_name(app_name)
                .engine_version(0)
                .api_version(vk::make_version(1, 0, 0));

            let mut surface_extensions = ash_window::enumerate_required_extensions(window).unwrap();
            info!("Surface required extensions: {:?}", surface_extensions);

            // Check for debug
            let supported_extensions = entry.enumerate_instance_extension_properties().unwrap();
            let debug_enabled = supported_extensions
                .iter()
                .any(|ext| CStr::from_ptr(ext.extension_name.as_ptr()) == DebugUtils::name());

            if debug_enabled {
                info!("Debug enabled");
                surface_extensions.push(DebugUtils::name());
            } else {
                info!("Debug not available");
            };

            let surface_extensions_raw = surface_extensions
                .iter()
                .map(|ext| ext.as_ptr())
                .collect::<Vec<_>>();

            // Enable validation layers
            let supported_layers = entry.enumerate_instance_layer_properties().unwrap();

            // FIXME
            let validation_layers = to_cstr!("VK_LAYER_KHRONOS_validation");
            let enabled_layers = if supported_layers
                .iter()
                .any(|layer| CStr::from_ptr(layer.layer_name.as_ptr()) == validation_layers)
            {
                [to_cstr!("VK_LAYER_KHRONOS_validation")]
            } else {
                [to_cstr!("")]
            };

            let enabled_layers_raw: Vec<_> =
                enabled_layers.iter().map(|layer| layer.as_ptr()).collect();
            /*
                        if enabled_layers.{
                        } else {
                            return Err("Validation layers requested but are not supported");
                        };
            */
            let mut create_info = vk::InstanceCreateInfo::builder()
                .application_info(&app_info)
                .enabled_extension_names(&surface_extensions_raw)
                .enabled_layer_names(&enabled_layers_raw);

            let mut debug_utils_messenger_info = vk::DebugUtilsMessengerCreateInfoEXT::builder()
                .message_severity(
                    vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                        | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                        | vk::DebugUtilsMessageSeverityFlagsEXT::INFO
                        | vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE,
                )
                .message_type(
                    vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                        | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                        | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                )
                .pfn_user_callback(Some(vulkan_debug_callback))
                .user_data(std::ptr::null_mut());
            if debug_enabled {
                create_info = create_info.push_next(&mut debug_utils_messenger_info);
            }

            let instance = entry.create_instance(&create_info, None).unwrap();

            let debug_utils;
            let debug_messenger;
            if debug_enabled {
                let utils = DebugUtils::new(&entry, &instance);
                debug_messenger = utils
                    .create_debug_utils_messenger(&debug_utils_messenger_info, None)
                    .unwrap();
                debug_utils = Some(utils);
            } else {
                debug_utils = None;
                debug_messenger = vk::DebugUtilsMessengerEXT::null();
            }

            // Physical device
            let surface = ash_window::create_surface(&entry, &instance, window, None).unwrap();
            let surface_loader = Surface::new(&entry, &instance);
            // FIXME: Only selects for graphics queues that also support the surface/present
            let (physical_device, queue_family_index) = instance
                .enumerate_physical_devices()
                .unwrap()
                .iter()
                .map(|device| {
                    instance
                        .get_physical_device_queue_family_properties(*device)
                        .iter()
                        .enumerate()
                        .filter_map(|(index, ref info)| {
                            let supports_graphics_and_surface =
                                info.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                                    && surface_loader
                                        .get_physical_device_surface_support(
                                            *device,
                                            index as u32,
                                            surface,
                                        )
                                        .unwrap();
                            if supports_graphics_and_surface {
                                Some((*device, index))
                            } else {
                                None
                            }
                        })
                        .next()
                })
                .filter_map(|v| v)
                .next()
                .unwrap();

            // Logical device
            let prios = [1.0];
            let queue_info = [vk::DeviceQueueCreateInfo::builder()
                .queue_family_index(queue_family_index as u32)
                .queue_priorities(&prios)
                .build()];

            let device_extensions = [Swapchain::name().as_ptr()];
            let device_features = vk::PhysicalDeviceFeatures::default();

            let device_create_info = vk::DeviceCreateInfo::builder()
                .queue_create_infos(&queue_info)
                .enabled_extension_names(&device_extensions)
                .enabled_features(&device_features);

            let device = instance.create_device(physical_device, &device_create_info, None).unwrap();

            // Queue
            let present_queue = device.get_device_queue(queue_family_index as u32, 0);

            // Swapchain
            let capabilities = surface_loader.get_physical_device_surface_capabilities(physical_device, surface).unwrap();

            let surface_formats = surface_loader.get_physical_device_surface_formats(physical_device, surface).unwrap();
            let surface_format = surface_formats
                .iter()
                .map(|f| f.format == vk::Format::B8G8R8A8_SRGB && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR)
                .next()
                .unwrap();


            let present_modes = surface_loader.get_physical_device_surface_present_modes(physical_device, surface).unwrap();


            Ok(Renderer {
                entry,
                instance,
                device,
                physical_device,
                queue_family_index: queue_family_index as u32,
                present_queue,
                surface_loader,
                surface,
                debug_utils,
                debug_messenger,
            })
        }
    }

    pub fn render(&mut self) {}
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().unwrap();
            if let Some(ref utils) = self.debug_utils {
                utils.destroy_debug_utils_messenger(self.debug_messenger, None);
            }
            self.device.destroy_device(None);
            self.surface_loader.destroy_surface(self.surface, None);
            self.instance.destroy_instance(None);
        }
    }
}

unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _user_data: *mut std::os::raw::c_void,
) -> vk::Bool32 {
    let callback_data = *p_callback_data;
    let message_id_number: i32 = callback_data.message_id_number as i32;

    let message_id_name = if callback_data.p_message_id_name.is_null() {
        Cow::from("")
    } else {
        CStr::from_ptr(callback_data.p_message_id_name).to_string_lossy()
    };

    let message = if callback_data.p_message.is_null() {
        Cow::from("")
    } else {
        CStr::from_ptr(callback_data.p_message).to_string_lossy()
    };

    info!(
        "{:?}:\n{:?} [{} ({})] : {}\n",
        message_severity,
        message_type,
        message_id_name,
        &message_id_number.to_string(),
        message,
    );

    vk::FALSE
}