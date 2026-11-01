//! Checks generated header and table files for unexpected regressions.

use std::fs;
use temp_dir::temp_dir;
use xbox_recomp2::DiskWriter;
use xbox_recomp2::config::{FunctionAddress, RecompilerConfig};
use xbox_recomp2::executable::{
    ConfigDefine, Executable, ImportToken, Section, SectionFlags, Symbol, ThunkAddress,
};

#[path = "common/temp_dir.rs"]
mod temp_dir;

/// Embeds the expected contents of `tests/goldens/<name>`.
macro_rules! golden {
    ($name:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/goldens/",
            $name
        ))
    };
}

/// The writer's output matches the golden files byte for byte:
///
/// - `x86_config.h`
/// - `x86_context.h`
/// - `x86_recomp_shared.h`
/// - `x86_func_mapping.cpp`
#[test]
fn writer_should_match_golden_files() {
    // Arrange.
    let dir = temp_dir("writer-goldens");
    let executable = golden_executable();
    let config = RecompilerConfig {
        directory_path: dir.to_string_lossy().into_owned().into(),
        out_directory_path: "x86".into(),
        function_hooks: [FunctionAddress(0x2345)].into(), // <- func_two may be replaced
        ..Default::default()
    };
    let mut writer = DiskWriter::new(&config).expect("create writer");
    let context_body = concat!(
        "struct CtxFixture { int field; };\n",
        "static_assert(sizeof(CtxFixture) == 4, \"fixture\");\n",
    );

    // Act.
    writer.write_config_header(&executable);
    writer
        .save(Some("x86_config.h"))
        .expect("save config header");

    writer.write_context_header(context_body.as_bytes());
    writer
        .save(Some("x86_context.h"))
        .expect("save context header");

    writer.write_shared_header(&executable, &config);
    writer
        .save(Some("x86_recomp_shared.h"))
        .expect("save shared header");

    writer.write_func_mapping(&executable);
    writer
        .save(Some("x86_func_mapping.cpp"))
        .expect("save func mapping");

    // Assert.
    let config_header = fs::read_to_string(dir.join("x86/x86_config.h")).unwrap();
    assert_eq!(config_header, golden!("config_header.txt"));

    let context_header = fs::read(dir.join("x86/x86_context.h")).unwrap();
    assert_eq!(context_header, golden!("context_header.txt").as_bytes());

    let shared_header = fs::read_to_string(dir.join("x86/x86_recomp_shared.h")).unwrap();
    assert_eq!(shared_header, golden!("shared_header.txt"));

    let func_mapping = fs::read_to_string(dir.join("x86/x86_func_mapping.cpp")).unwrap();
    assert_eq!(func_mapping, golden!("func_mapping.txt"));

    fs::remove_dir_all(dir).unwrap();
}

/// Builds the executable that the writer golden files describe.
fn golden_executable() -> Executable {
    let mut executable = Executable {
        base: 0x10000u64.into(),
        size_of_image: 0x8D3000,
        config_defines: vec![ConfigDefine {
            name: "X86_CODE_D3D8V",
            value: 5849,
        }],
        sections: vec![
            // Code at 0x1000..0x7E00.
            Section::new(
                "",
                0x1000u64, // base
                0x6E00,    // size
                SectionFlags::CODE,
                0, // file offset
            ),
            // Not code. Some arbitrary data at 0x9000.
            Section::new(
                "",
                0x9000u64, // base
                0x3000,    // size
                SectionFlags::from_bits_retain(0x40),
                0x6E00, // file offset
            ),
        ],
        ..Default::default()
    };

    executable.symbols.insert_batch([
        Symbol::function("func_two", 0x2345u64, 1),
        Symbol::function("__xbox_entry_point", 0x3333u64, 1),
        Symbol::function("FuncOne", 0x1100u64, 1),
        Symbol::function("__imp__DbgPrint", 0x9000u64, 1),
    ]);

    // Kernel import thunk at 0x9000, which the host always replaces.
    executable
        .imports
        .add_thunk(ThunkAddress(0x9000), ImportToken(0x8000_0008));

    executable
}
