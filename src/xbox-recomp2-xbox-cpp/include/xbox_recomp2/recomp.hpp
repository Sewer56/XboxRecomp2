#pragma once
#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <functional>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

// Opaque handle from xbox_recomp2.h.
struct XbExecutable;

namespace xbox_recomp2 {

struct RunError {
    enum class Code {
        /// Config or input loading failed.
        LoadError,
        /// Function discovery found nothing to recompile.
        NoDerivableFunctions,
        /// Discovery didn't produce a function at the executable's entry point.
        MissingEntryPoint,
        /// An output path is a symlink.
        OutputPathIsSymlink,
        /// Reading, writing or creating a path failed.
        IO,
        /// Bad argument passed to the binding.
        InvalidArgument,
        /// The Rust side panicked (release aborts).
        Panic,
    };

    Code code;

    /// The input or output path associated with the error, if any.
    std::filesystem::path path;

    /// The guest entry point address (MissingEntryPoint only).
    std::optional<std::uint64_t> address;

    /// Full error text from the recompiler.
    std::string message;

    [[nodiscard]] const std::string &toString() const { return message; }
};

/// Receives progress and diagnostic text.
using DiagSink = std::function<void(std::string_view)>;

/// Runs the recompiler with Xbox defaults.
///
/// @param input config TOML to load.
/// @param diag  receives progress and diagnostics. This might be empty.
/// @return nullopt on success, otherwise the error.
[[nodiscard]] std::optional<RunError> run(const std::filesystem::path &input,
                                          const DiagSink &diag = {});

struct Section {
    std::string name;
    std::uint64_t base;
    std::uint32_t size;
    std::uint32_t rawAddress;
    std::uint32_t rawSize;
};

/// A parsed Xbox executable (XBE).
class Executable {
public:
    /// Parses `data`. A failed parse gives an Executable with `empty() == true`.
    static Executable parse(std::span<const std::uint8_t> data);

    /// Whether parsing produced nothing.
    [[nodiscard]] bool empty() const;
    /// Guest address the executable is loaded at.
    [[nodiscard]] std::uint64_t base() const;
    [[nodiscard]] std::uint64_t entryPoint() const;
    /// Input file size in bytes.
    [[nodiscard]] std::uint32_t size() const;
    /// Size in guest memory.
    [[nodiscard]] std::uint64_t sizeOfImage() const;
    [[nodiscard]] std::vector<Section> sections() const;

private:
    struct Deleter {
        void operator()(XbExecutable *) const noexcept;
    };
 
    explicit Executable(XbExecutable *handle) : handle_(handle) {}
    std::unique_ptr<XbExecutable, Deleter> handle_;
};

} // namespace xbox_recomp2
