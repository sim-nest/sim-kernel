//! The native ABI: byte-frame and manifest transport types for dynamic libs.
//!
//! The kernel defines the versioned entrypoint, header, and call/response
//! frame contracts that native libraries are loaded across; it defines no
//! guest semantics beyond this transport.

use std::ffi::{CString, c_char, c_void};

/// Exported symbol name a v1 native dynamic library must expose.
pub const NATIVE_DYLIB_ENTRYPOINT_V1: &str = "sim_native_abi_v1";
/// Major ABI version of the v1 native library transport.
pub const NATIVE_LIB_ABI_V1_MAJOR: u16 = 1;
/// Minor ABI version of the v1 native library transport.
pub const NATIVE_LIB_ABI_V1_MINOR: u16 = 0;

/// Entry that creates a fresh guest instance, returning an opaque handle.
pub type NativeAbiInstantiate = unsafe extern "C" fn() -> *mut c_void;
/// Entry that destroys a guest instance handle.
pub type NativeAbiDestroyInstance = unsafe extern "C" fn(instance: *mut c_void);
/// Entry that returns the guest's encoded manifest as a call response.
pub type NativeAbiManifest = unsafe extern "C" fn(instance: *mut c_void) -> NativeAbiCallResponse;
/// Entry that invokes a named guest function with borrowed argument bytes.
pub type NativeAbiCall = unsafe extern "C" fn(
    instance: *mut c_void,
    function: *const c_char,
    args: NativeAbiBorrowedBytes,
) -> NativeAbiCallResponse;
/// Entry that frees bytes the guest previously handed back to the host.
pub type NativeAbiDestroyBytes = unsafe extern "C" fn(bytes: NativeAbiOwnedBytes);
/// Entry that frees an error the guest previously handed back to the host.
pub type NativeAbiDestroyError = unsafe extern "C" fn(error: *mut NativeAbiError);

/// Version header prefix shared by every native library ABI struct.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NativeLibAbiHeaderV1 {
    /// Byte size of the full ABI struct, for forward-compatible probing.
    pub struct_size: usize,
    /// Major ABI version the library was built against.
    pub abi_major: u16,
    /// Minor ABI version the library was built against.
    pub abi_minor: u16,
}

impl NativeLibAbiHeaderV1 {
    /// Builds a header from an explicit size and version pair.
    pub const fn new(struct_size: usize, abi_major: u16, abi_minor: u16) -> Self {
        Self {
            struct_size,
            abi_major,
            abi_minor,
        }
    }
}

/// The v1 native library vtable: header plus the entrypoint function table.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NativeLibAbiV1 {
    /// Byte size of this struct, matching [`NativeLibAbiHeaderV1::struct_size`].
    pub struct_size: usize,
    /// Major ABI version the library was built against.
    pub abi_major: u16,
    /// Minor ABI version the library was built against.
    pub abi_minor: u16,
    /// Instance-creation entry.
    pub instantiate: NativeAbiInstantiate,
    /// Instance-destruction entry.
    pub destroy_instance: NativeAbiDestroyInstance,
    /// Manifest-fetch entry.
    pub manifest: NativeAbiManifest,
    /// Function-call entry.
    pub call: NativeAbiCall,
    /// Byte-buffer free entry.
    pub destroy_bytes: NativeAbiDestroyBytes,
    /// Error free entry.
    pub destroy_error: NativeAbiDestroyError,
}

impl NativeLibAbiV1 {
    /// Byte size of the leading [`NativeLibAbiHeaderV1`] prefix.
    pub const HEADER_SIZE: usize = std::mem::size_of::<NativeLibAbiHeaderV1>();

    /// Builds a v1 vtable, stamping in the current size and version.
    pub const fn new(
        instantiate: NativeAbiInstantiate,
        destroy_instance: NativeAbiDestroyInstance,
        manifest: NativeAbiManifest,
        call: NativeAbiCall,
        destroy_bytes: NativeAbiDestroyBytes,
        destroy_error: NativeAbiDestroyError,
    ) -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>(),
            abi_major: NATIVE_LIB_ABI_V1_MAJOR,
            abi_minor: NATIVE_LIB_ABI_V1_MINOR,
            instantiate,
            destroy_instance,
            manifest,
            call,
            destroy_bytes,
            destroy_error,
        }
    }

    /// Extracts the version header prefix of this vtable.
    pub const fn header(&self) -> NativeLibAbiHeaderV1 {
        NativeLibAbiHeaderV1::new(self.struct_size, self.abi_major, self.abi_minor)
    }
}

/// A borrowed byte slice passed by raw pointer across the ABI boundary.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NativeAbiBorrowedBytes {
    /// Pointer to the first byte, or null when empty.
    pub ptr: *const u8,
    /// Number of bytes addressed by `ptr`.
    pub len: usize,
}

impl NativeAbiBorrowedBytes {
    /// The empty slice (null pointer, zero length).
    pub const fn empty() -> Self {
        Self {
            ptr: std::ptr::null(),
            len: 0,
        }
    }

    /// Borrows an existing slice for the duration of a single ABI call.
    ///
    /// # Examples
    ///
    /// ```
    /// use sim_kernel::NativeAbiBorrowedBytes;
    ///
    /// let payload = [1u8, 2, 3];
    /// let borrowed = NativeAbiBorrowedBytes::borrow(&payload);
    /// assert_eq!(borrowed.len, 3);
    /// assert!(!borrowed.ptr.is_null());
    ///
    /// let empty = NativeAbiBorrowedBytes::empty();
    /// assert_eq!(empty.len, 0);
    /// assert!(empty.ptr.is_null());
    /// ```
    pub fn borrow(bytes: &[u8]) -> Self {
        Self {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        }
    }
}

/// An owned byte buffer transferred across the ABI, freed by its allocator.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NativeAbiOwnedBytes {
    /// Pointer to the first byte, or null when empty.
    pub ptr: *mut u8,
    /// Number of valid bytes addressed by `ptr`.
    pub len: usize,
    /// Allocated capacity, needed to reconstruct the owning `Vec`.
    pub cap: usize,
}

impl NativeAbiOwnedBytes {
    /// The empty buffer (null pointer, zero length and capacity).
    pub const fn empty() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }
}

/// Result of a manifest or call entry: either owned bytes or an error.
#[repr(C)]
pub struct NativeAbiCallResponse {
    /// Success payload bytes; empty when `error` is set.
    pub bytes: NativeAbiOwnedBytes,
    /// Failure pointer, or null on success.
    pub error: *mut NativeAbiError,
}

impl NativeAbiCallResponse {
    /// Builds a successful response carrying the given bytes.
    pub const fn success(bytes: NativeAbiOwnedBytes) -> Self {
        Self {
            bytes,
            error: std::ptr::null_mut(),
        }
    }

    /// Builds a failed response carrying the given error pointer.
    pub const fn failure(error: *mut NativeAbiError) -> Self {
        Self {
            bytes: NativeAbiOwnedBytes::empty(),
            error,
        }
    }
}

/// An error returned across the ABI as a heap-allocated C string holder.
#[repr(C)]
pub struct NativeAbiError {
    /// NUL-terminated message owned by the allocating side.
    pub message: *mut c_char,
}

impl NativeAbiError {
    /// Heap-allocates an error, sanitizing interior NULs out of the message.
    pub fn boxed(message: impl Into<String>) -> *mut Self {
        let sanitized = message.into().replace('\0', " ");
        let message = CString::new(sanitized)
            .expect("sanitized native ABI error strings must not contain interior NULs");
        Box::into_raw(Box::new(Self {
            message: message.into_raw(),
        }))
    }
}

/// Transfers ownership of a `Vec<u8>` into an ABI owned-bytes descriptor.
///
/// # Examples
///
/// ```
/// use sim_kernel::native_abi_owned_bytes;
///
/// let owned = native_abi_owned_bytes(vec![7u8, 8, 9]);
/// assert_eq!(owned.len, 3);
/// assert!(owned.cap >= owned.len);
///
/// // The descriptor owns the allocation; reclaim it to avoid a leak.
/// let reclaimed = unsafe { Vec::from_raw_parts(owned.ptr, owned.len, owned.cap) };
/// assert_eq!(reclaimed, vec![7u8, 8, 9]);
/// ```
pub fn native_abi_owned_bytes(mut bytes: Vec<u8>) -> NativeAbiOwnedBytes {
    let result = NativeAbiOwnedBytes {
        ptr: bytes.as_mut_ptr(),
        len: bytes.len(),
        cap: bytes.capacity(),
    };
    std::mem::forget(bytes);
    result
}
