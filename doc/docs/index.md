---
hide:
  - toc
---

<h1 class="visually-hidden">XboxRecomp2</h1>

<div align="center">
<pre class="ascii-logo">
                                                                                      
                                                                                 .-''-.     
         ▄▄▀                                                                   .' .-.  )    
   ▄▄▀▄██▀                                                                    / .'  / /     
  ▄█ ███                                                                     (_/   / /      
 █▓██▓█ ▄▄███  ▐▄• ▄ ▄▄▄▄·       ▐▄• ▄ ▄▄▄  ▄▄▄ . ▄▄·       • ▌ ▄ ·.  ▄▄▄·       / /       
 █▓█████▓█▀    █▌█▌▪▐█ ▀█▪▪      █▌█▌▪▀▄ █·▀▄.▀·▐█ ▌▪▪     ·██ ▐███▪▐█ ▄█       / /        
█▓▓▀ ▄▄▄██▄▄   ·██· ▐█▀▀█▄ ▄█▀▄  ·██· ▐▀▀▄ ▐▀▀▪▄██ ▄▄ ▄█▀▄ ▐█ ▌▐▌▐█· ██▀·      . '         
█▒▒█▄▄██ █    ▪▐█·█▌██▄▪▐█▐█▌.▐▌▪▐█·█▌▐█•█▌▐█▄▄▌▐███▌▐█▌.▐▌██ ██▌▐█▌▐█▪·•     / /    _.-') 
█▀▓▒▒▓▓▀▀     •▀▀ ▀▀·▀▀▀▀  ▀█▄▀▪•▀▀ ▀▀.▀  ▀ ▀▀▀ ·▀▀▀  ▀█▄▀▪▀▀  █▪▀▀▀.▀      .' '  _.'.-''  
   ▀▓▓▄                                                                    /  /.-'_.'      
     ▀▀██▄▄▄                                                              /    _.'         
                                                                         ( _.-'            
                                                                          
</pre>
</div>

<center>
A Sewer56 Tech Demo<br/><br/>

XboxRecomp2 translates original Xbox x86 game code into C++ source.<br/>
And makes it go brr~ 🦔 🏎️
</center>

![](./images/shadowrecomp.lossy.q85.avif){: width="960" style="display:block;margin:0 auto" }

<center>2500FPS w/ 9800X3D, Shadow The Hedgehog (Xbox) on Linux</center>

For many reasons, ***x86 is a notoriously difficult architecture***
to recompile; ***especially at speed***.<br/>
Most recompilers do 1:1 code translation; but that simply doesn't cut it here.

XboxRecomp2 achieves around 2.5x the performance of a raw 1:1 ASM -> C translation.<br/>
Through a series of speed hacks, heavy optimisation, and intermediate code rewrites.

You can learn about [How It Works] in this detailed write-up.

## Source Code Structure

!!! info "How the internal code is arranged."

```text
         +--------------+
         | cli          |     Entry point (command-line binary)
         +------+-------+
                |
                v
     +---------------------+
     | xbox-recomp2-xbox   |  XBE + Xbox platform
     +----------+----------+
                |
                v
     +---------------------+
     | xbox-recomp2        |  Core recompiler
     +------+-------+------+
            |       |
            |       +----------------------+
            v                              v
  +---------------------+     +-----------------------------+
  | xbox-recomp2-       |     | xbox-recomp2-recomp-config  |
  | executable          |     | (recompConfig.toml parser)  |
  +---------------------+     +-----------------------------+
```

Arrows point from a crate (project) to the other crates it uses.

- **xbox-recomp2**: Core library (recompiler pipeline)
- **xbox-recomp2-executable**: Allows you to add executable formats (e.g. `.xbe`, `.exe`)
- **xbox-recomp2-xbox**: Xbox support: XBE loading, memory access routing etc.
- **xbox-recomp2-recomp-config**: Parser for `recompConfig.toml`.
- **cli**: Command-line interface.

The XboxRecomp2 recompiler supports most of the x86 instruction set
required to run games built for the original Xbox. (Pentium III)

Also taken extra care to carefully carve out the Xbox-specific parts
of the recompiler into their own, dedicated modules and crates.

In other words, you can make this recompile Win32, Linux, Chihiro,
SEGA Lindbergh or other 32-bit x86-based platforms.<br/>
That's an exercise for the reader 😉.

!!! warning "Warning: Imperfect Code"

    Due to time constraints, some parts of source code aren't as polished as
    I'd like.

    Quite frankly, I spent 12h everyday coding, for 3 months straight; between
    paid work and this recompiler.<br/> For our Nov15 Shadow05 release.

    I provide this recompiler to help you port awesome Xbox games to the PC.<br/>
    That said, I got too many other projects of my own- please fork
    this if you plan to do ***significant*** work on this.

## Why I wrote it

!!! danger "Black Hedgehog go brr!"

    Moved to [why I wrote it](./why-i-wrote-it.md)

    This requires some storytelling.

## Developer Manual

For step-by-step development guidance, see the [Developer Manual].

## Contributing

We welcome contributions! See the [Contributing Guide] for details.

## License

Licensed under [MIT].

## Greetz

- Sora & Skyth: Original XboxRecomp & XenonRecomp which were used as reference.
- CXBX-Reloaded & Co: Reference code for kernel thunk handling & other similar shenianigans.
- Caustik: (OG CXBX Dev) [XBE File Format Reference](<https://xbdev.net/fileformats/xbe/NonOfficialXBEFileFormat.pdf>) 
- Font: Elite (*from TheDraw*, Author Unknown)

[XboxRecomp]: https://github.com/FoxWorkshop/XboxRecomp#instructions
[How It Works]: how-it-works.md
[Developer Manual]: https://reloaded-project.github.io/reloaded-templates-rust/manual/
[Contributing Guide]: https://reloaded-project.github.io/reloaded-templates-rust/manual/#contributing
[MIT]: ../../LICENSE