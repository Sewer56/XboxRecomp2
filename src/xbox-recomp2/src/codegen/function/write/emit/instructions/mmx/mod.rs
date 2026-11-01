//! MMX instructions. Most also handle their XMM form.

mod dword_unpack;
pub(super) mod emms;
pub(super) mod movd;
pub(super) mod movntq;
pub(super) mod movq;
pub(super) mod packssdw;
pub(super) mod packuswb;
pub(super) mod paddw;
pub(super) mod pavgb;
pub(super) mod pextrw;
pub(super) mod pshufw;
pub(super) mod psraw;
pub(super) mod psrlq;
pub(super) mod punpckhdq;
pub(super) mod punpckhwd;
pub(super) mod punpckldq;
pub(super) mod punpcklwd;
mod word_unpack;
