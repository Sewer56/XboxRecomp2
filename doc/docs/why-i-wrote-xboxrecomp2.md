# Why I wrote XboxRecomp2

!!! info "This requires some storytelling"

    I'll tell a bit of my side 😉; word is Sora is planning a blog of their own.

Back in March 2025; [Sora] started working on
ShadowRecomp; the world's first recompilation of an original Xbox game.

When the project started, I (Sewer) helped a tiny bit with getting that off the
ground.<br/>Between [Skyth] (of Unleashed Recompiled fame), and myself, we taught
Sora a little about both the technique of Static Recompilation, and the
intricacies of x86.

Lo and behold. [ShadowRecomp] was born. Gradual steps were made, and after around
1 year of on-and-off development, Sora had a playable version of the game;
even if very, very far from perfect (bugs galore!).

!!! question "Why Xbox?"

    (Hasn't everything always been done on GCN version of the game?)

    Well- I was kinda whispering into Sora's ear to do Xbox 😅.

    I told him:

    - Map the code sections in memory
    - Hook the Xbox specific stuff
    - Replace kernel stuff with Windows equivalents
    - You have very close mappings with 'ntdll', 'DirectX 8.1', etc. almost 1:1
    - Use existing hooking libraries and mod it like real PC game.
    - Already Little Endian (convenient)

    Basically HLE the Xbox OS, the OG CXBX approach.

    In the end, Sora went for Static Recomp approach; still Xbox.<br/>
    Similar but different. You'll have to wait for that blog post :P

## Quick Speedrun

!!! info "This was all Sora, solo tanking it on-and-off for a year"

    And he learned D3D12 and a lot of x86 while at it- legendary.

<figure markdown>
  ![](./images/shadow_title_02_oct_2025.lossy.q85.avif){: width="655"}
  <figcaption>02 Oct 2025</figcaption>
</figure>

<figure markdown>
  ![](./images/shadow_title_18_oct_2025.lossy.q85.avif){: width="640"}
  <figcaption>18 Oct 2025</figcaption>
</figure>

<figure markdown>
  ![](./images/shadow_ingame_nov_13.lossy.q85.avif){: width="640"}
  <figcaption>13 Nov 2025</figcaption>
</figure>

<figure markdown>
  ![](./images/shadow_ingame_16_dec_2025.lossy.q85.avif){: width="642"}
  <figcaption>16 Dec 2025</figcaption>
</figure>

<figure markdown>
  ![](./images/shadow_ingame_20_jan_2026.lossy.q85.avif){: width="642"}
  <figcaption>20 Jan 2026</figcaption>
</figure>

<figure markdown>
  ![](./images/shadow_ingame_11_apr_2026.lossy.q85.avif){: width="642"}
  <figcaption>11 Apr 2026</figcaption>
</figure>

You get the idea.

By late April, Sora got the game to a pretty playable state; and with extra
contributions from [dreamsyntax] we also had a working Linux build.

While the project could have been released back then as the
*'World's first Xbox recompilation'*... however.

Much like [DarioSamo], one of the old grandfathers of the 3D Sonic modding scene
& the developer of the graphics library code used in many recomps, we're
[not interested in a race to the bottom] for 10 minutes of screen fame.

(At that time, it still wasn't any better than playing GCN on Emulator.)

!!! question "Did you know?"

    ![](./images/cxbx.lossy.q85.avif)

    CXBX-Reloaded was the reference for a lot of things in ShadowRecomp.<br/>
    Does this image look familiar?<br/>

    Yup- matches Nov 13. This is a recomp of a game broken on emu.

    PS. (We do have a patch- but, CXBX-Reloaded restarted development from scratch- awkward...)

## Performance Issues

!!! note "During those days, ShadowRecomp had a fair bit of performance issues"

    - Vulkan on Nvidia unusual perf issues
    - High CPU Usage on Events
    - Shader Compilation Stutters
    - Doubled & Inaccurate Frame Pacing
    - Particles...

    And many... many more.

It was Summer Time. And I personally had some *bad health issues*.
Passed out once, heatwaves, a hospital visit, became diabetic- etc.

Coincidentally, it was the longest break from programming I've had in >3 years.

So anyway, once I got back to Reloaded dev shenanigans; I opened a PR on a
project and had to wait a bit. So I decided to clone and faff around with Recomp.
A quick Nix flake here, build there. And we ballin'.

Figured; might aswell throw in as many improvements as I can-.

!!! info "And thus the grind started..."

<div class="grid" markdown>
<figure markdown>
  ![](./images/shadow_23_aug_2026.lossy.q85.avif)
  <figcaption>23 Aug 2026 (~400FPS)<br/>[Early perf work].</figcaption>
</figure>
<figure markdown>
  ![](./images/shadow_31_oct_2026.lossy.q85.avif)
  <figcaption>31 Oct 2026 (~1300FPS)<br/>XboxRecomp2 Beta.</figcaption>
</figure>
</div>

At first I fixed framepacing...<br/>
Then FPS dropping (<120) and game slowdown on Particles/Transparent Objects...<br/>
Then a lot of smaller things...<br/>

Until eventually...
I became CPU bottlenecked to the point where the game code itself became the bottleneck.
A little weekend detour became a daily grind for 3 months straight.

*XboxRecomp2 was born.*<br/>
An improvement in every metric.

!!! info "But it wasn't cheap."

    12 hours of code every day, between work and XboxRecomp2.<br/>
    Now it's time for a break 😉; I'll enjoy some well-deserved rest.

[Sora]: https://github.com/sora-yx
[Skyth]: https://github.com/blueskythlikesclouds
[clean room]: https://en.wikipedia.org/wiki/Clean-room_design
[ShadowRecomp]: https://github.com/FoxWorkshop/ShadowRecomp
[not interested in a race to the bottom]: https://bsky.app/profile/dariosamo.bsky.social/post/3mwtxn4m2gd2h
[DarioSamo]: https://github.com/dariosamo
[dreamsyntax]: https://github.com/dreamsyntax