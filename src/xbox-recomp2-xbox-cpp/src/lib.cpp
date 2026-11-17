#include "xbox_recomp2/recomp.hpp"
#include "xbox_recomp2/xbox_recomp2.h"

#include <exception>
#include <string_view>

namespace xbox_recomp2 {

namespace {

struct DiagContext {
    const DiagSink *sink;
    std::exception_ptr error;
}

void diagTrampoline(const std::uint8_t *data, std::size_t len, void *user) noexcept {
    auto &ctx = *static_cast<DiagContext *>(user);

    if (ctx.error) return;

    try {
        (*ctx.sink)(std::string_view(reinterpret_cast<const char *>(data), len));
    } catch (...) {
        ctx.error = std::current_exception();
    }
 }

RunError::Code toCode(int code) {
    switch (code) {
        case XB_ERR_LOAD: return RunError::Code::LoadError;
        case XB_ERR_NO_DERIVABLE_FUNCTIONS: return RunError::Code::NoDerivableFunctions;
        case XB_ERR_MISSING_ENTRY_POINT: return RunError::Code::MissingEntryPoint;
        case XB_ERR_OUTPUT_PATH_IS_SYMLINK: return RunError::Code::OutputPathIsSymlink;
        case XB_ERR_IO: return RunError::Code::IO;
        case XB_ERR_PANIC: return RunError::Code::Panic;
        default: return RunError::Code::InvalidArgument;
    }
}

} // namespace

std::optional<RunError> run(const std::filesystem::path &input, const DiagSink &diag) {
    // The Rust code expects UTF-8
    const std::u8string u8 = input.u8string();
    const std::string utf8(reinterpret_cast<const char *>(u8.data()), u8.size());

    DiagContext ctx{&diag, nullptr};
    XbRunError err{};
    const int rc = xb_run(utf8.c_str(), diag ? diagTrampoline : nullptr,
                          diag ? &ctx : nullptr, &err);

    if (ctx.error) {
        xb_error_free(&err);
        std::rethrow_exception(ctx.error);
    }

    if (rc == XB_OK) return std::nullopt;

    RunError out{
        .code = toCode(rc),
        .path = err.path ? std::filesystem::path(std::u8string(
                               reinterpret_cast<const char8_t *>(err.path)))
                         : std::filesystem::path{},
        .address = err.has_address ? std::optional<std::uint64_t>(err.address) : std::nullopt,
        .message = err.message ? err.message : "",
    };

    xb_error_free(&err);
    return out;
}

void Executable::Deleter::operator()(XbExecutable *p) const noexcept { xb_executable_free(p); }

Executable Executable::parse(std::span<const std::uint8_t> data) {
    return Executable(xb_parse_executable(data.data(), data.size()));
}

bool Executable::empty() const { return xb_executable_is_empty(handle_.get()); }
std::uint64_t Executable::base() const { return xb_executable_base(handle_.get()); }
std::uint64_t Executable::entryPoint() const { return xb_executable_entry_point(handle_.get()); }
std::uint32_t Executable::size() const { return xb_executable_size(handle_.get()); }
std::uint64_t Executable::sizeOfImage() const { return xb_executable_size_of_image(handle_.get()); }

std::vector<Section> Executable::sections() const {
    std::vector<Section> out;
    const std::size_t sectionCount = xb_executable_section_count(handle_.get());

    out.reserve(sectionCount);

    for (std::size_t i = 0; i < sectionCount; ++i) {
        XbSection s{};
        if (!xb_executable_section(handle_.get(), i, &s))
            break;

        out.push_back({std::string(s.name, s.name_len), s.base, s.size, s.raw_address, s.raw_size});
    }

    return out;
}

} // namespace xbox_recomp2
