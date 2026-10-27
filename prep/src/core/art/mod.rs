//! `core::art`'s pure files (`image` + `texpresso`): jacket composition,
//! the stock-shaped thumbnail DDS, the embedded placeholder and source
//! classification. The DLL-only arc containers stay in the DLL.
#[path = "../../../../src/core/art/arcpack.rs"]
pub mod arcpack;
#[path = "../../../../src/core/art/jacket.rs"]
pub mod jacket;
#[path = "../../../../src/core/art/placeholder.rs"]
pub mod placeholder;
#[path = "../../../../src/core/art/source.rs"]
pub mod source;
#[path = "../../../../src/core/art/thumbnail.rs"]
pub mod thumbnail;

pub use arcpack::thumbnail_arc;
pub use placeholder::placeholder_jacket;
pub use thumbnail::thumbnail_dds;
