//! Blurman patch: Mica and Acrylic that keep their active look while the window is not in front.
//!
//! WinUI's `MicaBackdrop` and `DesktopAcrylicBackdrop` follow the window's activation and turn to
//! a flat colour when another window is active. Blurman's window shows its glass the whole time
//! you use the apps it frosts, so this drives the backdrop controllers directly, with a
//! configuration that always says the input is active. The bindings below were generated with
//! windows-bindgen from the Windows App SDK metadata Reactor itself is built from.

use super::native;
use crate::{WindowBackdrop, WindowTheme};
use windows_core::Interface;

#[allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code, clippy::all)]
mod bindings {
    #[repr(transparent)]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct DesktopAcrylicController(windows_core::IUnknown);
    windows_core::imp::interface_hierarchy!(
        DesktopAcrylicController,
        windows_core::IUnknown,
        windows_core::IInspectable
    );
    impl DesktopAcrylicController {
        pub(crate) fn new() -> windows_core::Result<Self> {
            Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
        }
        fn IActivationFactory<
            R,
            F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
        >(
            callback: F,
        ) -> windows_core::Result<R> {
            static SHARED: windows_core::imp::FactoryCache<
                DesktopAcrylicController,
                windows_core::imp::IGenericFactory,
            > = windows_core::imp::FactoryCache::new();
            SHARED.call(callback)
        }
    }
    impl windows_core::RuntimeType for DesktopAcrylicController {
        const SIGNATURE: windows_core::imp::ConstBuffer =
            windows_core::imp::ConstBuffer::for_class::<Self, IDesktopAcrylicController>();
    }
    unsafe impl windows_core::Interface for DesktopAcrylicController {
        type Vtable = <IDesktopAcrylicController as windows_core::Interface>::Vtable;
        const IID: windows_core::GUID = <IDesktopAcrylicController as windows_core::Interface>::IID;
        const INSPECTABLE: bool = true;
    }
    impl core::ops::Deref for DesktopAcrylicController {
        type Target = IDesktopAcrylicController;
        fn deref(&self) -> &Self::Target {
            unsafe { core::mem::transmute(self) }
        }
    }
    impl windows_core::RuntimeName for DesktopAcrylicController {
        const NAME: &'static str = "Microsoft.UI.Composition.SystemBackdrops.DesktopAcrylicController";
    }
    unsafe impl Send for DesktopAcrylicController {}
    unsafe impl Sync for DesktopAcrylicController {}
    windows_core::imp::define_winrt_interface!(
        IClosable,
        IClosable_Vtbl,
        0x30d5a829_7fa4_4026_83bb_d75bae4ea99e
    );
    windows_core::imp::interface_hierarchy!(
        IClosable,
        windows_core::IUnknown,
        windows_core::IInspectable
    );
    impl IClosable {
        pub(crate) fn Close(&self) -> windows_core::Result<()> {
            unsafe {
                (windows_core::Interface::vtable(self).Close)(windows_core::Interface::as_raw(self))
                    .ok()
            }
        }
    }
    #[repr(C)]
    pub struct IClosable_Vtbl {
        pub base__: windows_core::IInspectable_Vtbl,
        pub Close: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    }
    windows_core::imp::define_winrt_interface!(
        ICompositionSupportsSystemBackdrop,
        ICompositionSupportsSystemBackdrop_Vtbl,
        0x397dafe4_b6c2_5bb9_951d_f5707de8b7bc
    );
    windows_core::imp::interface_hierarchy!(
        ICompositionSupportsSystemBackdrop,
        windows_core::IUnknown,
        windows_core::IInspectable
    );
    #[repr(C)]
    pub struct ICompositionSupportsSystemBackdrop_Vtbl {
        pub base__: windows_core::IInspectable_Vtbl,
    }
    windows_core::imp::define_winrt_interface!(
        IDesktopAcrylicController,
        IDesktopAcrylicController_Vtbl,
        0x7c20a6af_8eb3_5f08_bdfc_6d35e35dfe45
    );
    #[repr(C)]
    pub struct IDesktopAcrylicController_Vtbl {
        pub base__: windows_core::IInspectable_Vtbl,
    }
    windows_core::imp::define_winrt_interface!(
        IMicaController,
        IMicaController_Vtbl,
        0x2de996a9_0a2a_5889_a89c_1f84060a8cab
    );
    #[repr(C)]
    pub struct IMicaController_Vtbl {
        pub base__: windows_core::IInspectable_Vtbl,
    }
    windows_core::imp::define_winrt_interface!(
        IMicaController2,
        IMicaController2_Vtbl,
        0xf1ed4a52_d9ca_506e_9586_caaefd3aa971
    );
    impl IMicaController2 {
        pub(crate) fn SetKind(&self, value: MicaKind) -> windows_core::Result<()> {
            unsafe {
                (windows_core::Interface::vtable(self).SetKind)(
                    windows_core::Interface::as_raw(self),
                    value,
                )
                .ok()
            }
        }
    }
    #[repr(C)]
    pub struct IMicaController2_Vtbl {
        pub base__: windows_core::IInspectable_Vtbl,
        Kind: usize,
        pub SetKind:
            unsafe extern "system" fn(*mut core::ffi::c_void, MicaKind) -> windows_core::HRESULT,
    }
    windows_core::imp::define_winrt_interface!(
        ISystemBackdropConfiguration,
        ISystemBackdropConfiguration_Vtbl,
        0xebcce1b9_0e0c_5431_ab0e_00f3f0669962
    );
    impl ISystemBackdropConfiguration {
        pub(crate) fn SetIsInputActive(&self, value: bool) -> windows_core::Result<()> {
            unsafe {
                (windows_core::Interface::vtable(self).SetIsInputActive)(
                    windows_core::Interface::as_raw(self),
                    value,
                )
                .ok()
            }
        }
        pub(crate) fn SetTheme(&self, value: SystemBackdropTheme) -> windows_core::Result<()> {
            unsafe {
                (windows_core::Interface::vtable(self).SetTheme)(
                    windows_core::Interface::as_raw(self),
                    value,
                )
                .ok()
            }
        }
    }
    #[repr(C)]
    pub struct ISystemBackdropConfiguration_Vtbl {
        pub base__: windows_core::IInspectable_Vtbl,
        HighContrastBackgroundColor: usize,
        SetHighContrastBackgroundColor: usize,
        IsHighContrast: usize,
        SetIsHighContrast: usize,
        IsInputActive: usize,
        pub SetIsInputActive:
            unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
        Theme: usize,
        pub SetTheme: unsafe extern "system" fn(
            *mut core::ffi::c_void,
            SystemBackdropTheme,
        ) -> windows_core::HRESULT,
    }
    windows_core::imp::define_winrt_interface!(
        ISystemBackdropControllerWithTargets,
        ISystemBackdropControllerWithTargets_Vtbl,
        0x9c56fe7c_98eb_5f89_ad97_dad57fc30c8c
    );
    windows_core::imp::interface_hierarchy!(
        ISystemBackdropControllerWithTargets,
        windows_core::IUnknown,
        windows_core::IInspectable
    );
    impl ISystemBackdropControllerWithTargets {
        pub(crate) fn AddSystemBackdropTarget<P0>(
            &self,
            systembackdroptarget: P0,
        ) -> windows_core::Result<bool>
        where
            P0: windows_core::Param<ICompositionSupportsSystemBackdrop>,
        {
            unsafe {
                let mut result__ = core::mem::zeroed();
                (windows_core::Interface::vtable(self).AddSystemBackdropTarget)(
                    windows_core::Interface::as_raw(self),
                    systembackdroptarget.param().abi(),
                    &mut result__,
                )
                .map(|| result__)
            }
        }
        pub(crate) fn RemoveAllSystemBackdropTargets(&self) -> windows_core::Result<()> {
            unsafe {
                (windows_core::Interface::vtable(self).RemoveAllSystemBackdropTargets)(
                    windows_core::Interface::as_raw(self),
                )
                .ok()
            }
        }
        pub(crate) fn SetSystemBackdropConfiguration<P0>(
            &self,
            configuration: P0,
        ) -> windows_core::Result<()>
        where
            P0: windows_core::Param<SystemBackdropConfiguration>,
        {
            unsafe {
                (windows_core::Interface::vtable(self).SetSystemBackdropConfiguration)(
                    windows_core::Interface::as_raw(self),
                    configuration.param().abi(),
                )
                .ok()
            }
        }
    }
    #[repr(C)]
    pub struct ISystemBackdropControllerWithTargets_Vtbl {
        pub base__: windows_core::IInspectable_Vtbl,
        State: usize,
        pub AddSystemBackdropTarget: unsafe extern "system" fn(
            *mut core::ffi::c_void,
            *mut core::ffi::c_void,
            *mut bool,
        ) -> windows_core::HRESULT,
        pub RemoveAllSystemBackdropTargets:
            unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
        RemoveSystemBackdropTarget: usize,
        pub SetSystemBackdropConfiguration: unsafe extern "system" fn(
            *mut core::ffi::c_void,
            *mut core::ffi::c_void,
        ) -> windows_core::HRESULT,
    }
    #[repr(transparent)]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct MicaController(windows_core::IUnknown);
    windows_core::imp::interface_hierarchy!(
        MicaController,
        windows_core::IUnknown,
        windows_core::IInspectable
    );
    impl MicaController {
        pub(crate) fn new() -> windows_core::Result<Self> {
            Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
        }
        fn IActivationFactory<
            R,
            F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
        >(
            callback: F,
        ) -> windows_core::Result<R> {
            static SHARED: windows_core::imp::FactoryCache<
                MicaController,
                windows_core::imp::IGenericFactory,
            > = windows_core::imp::FactoryCache::new();
            SHARED.call(callback)
        }
    }
    impl windows_core::RuntimeType for MicaController {
        const SIGNATURE: windows_core::imp::ConstBuffer =
            windows_core::imp::ConstBuffer::for_class::<Self, IMicaController>();
    }
    unsafe impl windows_core::Interface for MicaController {
        type Vtable = <IMicaController as windows_core::Interface>::Vtable;
        const IID: windows_core::GUID = <IMicaController as windows_core::Interface>::IID;
        const INSPECTABLE: bool = true;
    }
    impl core::ops::Deref for MicaController {
        type Target = IMicaController;
        fn deref(&self) -> &Self::Target {
            unsafe { core::mem::transmute(self) }
        }
    }
    impl windows_core::RuntimeName for MicaController {
        const NAME: &'static str = "Microsoft.UI.Composition.SystemBackdrops.MicaController";
    }
    unsafe impl Send for MicaController {}
    unsafe impl Sync for MicaController {}
    #[repr(transparent)]
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub struct MicaKind(pub i32);
    impl MicaKind {
        pub const Base: Self = Self(0);
        pub const BaseAlt: Self = Self(1);
    }
    impl windows_core::imp::TypeKind for MicaKind {
        type TypeKind = windows_core::imp::CopyType;
    }
    impl windows_core::RuntimeType for MicaKind {
        const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
            b"enum(Microsoft.UI.Composition.SystemBackdrops.MicaKind;i4)",
        );
        const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
            b"Microsoft.UI.Composition.SystemBackdrops.MicaKind",
        );
    }
    #[repr(transparent)]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct SystemBackdropConfiguration(windows_core::IUnknown);
    windows_core::imp::interface_hierarchy!(
        SystemBackdropConfiguration,
        windows_core::IUnknown,
        windows_core::IInspectable
    );
    impl SystemBackdropConfiguration {
        pub(crate) fn new() -> windows_core::Result<Self> {
            Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
        }
        fn IActivationFactory<
            R,
            F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
        >(
            callback: F,
        ) -> windows_core::Result<R> {
            static SHARED: windows_core::imp::FactoryCache<
                SystemBackdropConfiguration,
                windows_core::imp::IGenericFactory,
            > = windows_core::imp::FactoryCache::new();
            SHARED.call(callback)
        }
    }
    impl windows_core::RuntimeType for SystemBackdropConfiguration {
        const SIGNATURE: windows_core::imp::ConstBuffer =
            windows_core::imp::ConstBuffer::for_class::<Self, ISystemBackdropConfiguration>();
    }
    unsafe impl windows_core::Interface for SystemBackdropConfiguration {
        type Vtable = <ISystemBackdropConfiguration as windows_core::Interface>::Vtable;
        const IID: windows_core::GUID = <ISystemBackdropConfiguration as windows_core::Interface>::IID;
        const INSPECTABLE: bool = true;
    }
    impl core::ops::Deref for SystemBackdropConfiguration {
        type Target = ISystemBackdropConfiguration;
        fn deref(&self) -> &Self::Target {
            unsafe { core::mem::transmute(self) }
        }
    }
    impl windows_core::RuntimeName for SystemBackdropConfiguration {
        const NAME: &'static str =
            "Microsoft.UI.Composition.SystemBackdrops.SystemBackdropConfiguration";
    }
    unsafe impl Send for SystemBackdropConfiguration {}
    unsafe impl Sync for SystemBackdropConfiguration {}
    #[repr(transparent)]
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub struct SystemBackdropTheme(pub i32);
    impl SystemBackdropTheme {
        pub const Default: Self = Self(0);
        pub const Light: Self = Self(1);
        pub const Dark: Self = Self(2);
    }
    impl windows_core::imp::TypeKind for SystemBackdropTheme {
        type TypeKind = windows_core::imp::CopyType;
    }
    impl windows_core::RuntimeType for SystemBackdropTheme {
        const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
            b"enum(Microsoft.UI.Composition.SystemBackdrops.SystemBackdropTheme;i4)",
        );
        const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
            b"Microsoft.UI.Composition.SystemBackdrops.SystemBackdropTheme",
        );
    }
}

use bindings::*;

/// A backdrop controller on one window. Dropping it takes the backdrop off the window.
pub(super) struct ActiveBackdrop {
    controller: ISystemBackdropControllerWithTargets,
    configuration: SystemBackdropConfiguration,
}

impl ActiveBackdrop {
    pub(super) fn new(
        window: &native::Window,
        backdrop: WindowBackdrop,
        theme: WindowTheme,
    ) -> windows_core::Result<Self> {
        let configuration = SystemBackdropConfiguration::new()?;
        configuration.SetIsInputActive(true)?;
        configuration.SetTheme(backdrop_theme(theme))?;
        let controller: ISystemBackdropControllerWithTargets = match backdrop {
            WindowBackdrop::Acrylic => DesktopAcrylicController::new()?.cast()?,
            _ => {
                let mica = MicaController::new()?;
                mica.cast::<IMicaController2>()?.SetKind(if backdrop == WindowBackdrop::MicaAlt {
                    MicaKind::BaseAlt
                } else {
                    MicaKind::Base
                })?;
                mica.cast()?
            }
        };
        controller.SetSystemBackdropConfiguration(&configuration)?;
        // Windows that cannot draw this backdrop (Mica before Windows 11) say so here rather than
        // failing, and the caller falls back to WinUI's own backdrop.
        if !controller.AddSystemBackdropTarget(&window.cast::<ICompositionSupportsSystemBackdrop>()?)? {
            return Err(windows_core::Error::from_hresult(windows_core::HRESULT(
                0x80004001u32 as i32, // E_NOTIMPL
            )));
        }
        Ok(Self {
            controller,
            configuration,
        })
    }

    pub(super) fn set_theme(&self, theme: WindowTheme) -> windows_core::Result<()> {
        self.configuration.SetTheme(backdrop_theme(theme))
    }
}

impl Drop for ActiveBackdrop {
    fn drop(&mut self) {
        let _ = self.controller.RemoveAllSystemBackdropTargets();
        if let Ok(closable) = self.controller.cast::<IClosable>() {
            let _ = closable.Close();
        }
    }
}

fn backdrop_theme(theme: WindowTheme) -> SystemBackdropTheme {
    match theme {
        WindowTheme::System => SystemBackdropTheme::Default,
        WindowTheme::Light => SystemBackdropTheme::Light,
        WindowTheme::Dark => SystemBackdropTheme::Dark,
    }
}
