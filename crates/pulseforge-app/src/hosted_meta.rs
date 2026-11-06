//! The cached static data read from each hosted plug-in.


pub(crate) struct HostedMeta {
    pub(crate) info: pulseforge_hosting::BundleInfo,
    pub(crate) parameters: Vec<pulseforge_hosting::ParameterInfo>,
    pub(crate) editor_size: (u32, u32),
}
