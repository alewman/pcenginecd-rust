# Third-party notices

pcenginecd-rust is MIT licensed (LICENSE). It is transcribed from pcenginecd-python, which is ours and
MIT, and contains no code from the GPL or LGPL emulators the comments cite; see pcengine-python's
`docs/provenance.md` for the rule and the check.

`src/seek.rs` (the zone table and the shape of the seek-time model) is transcribed from
pcenginecd-python's `src/pcenginecd_python/seek.py`, which is derived from Dave Shadoff's
`seektime.c` and his measurements (https://github.com/pce-devel/PCECD_seek). They are MIT licensed:

```
MIT License

Copyright (c) 2019 David Shadoff

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## MAME (BSD-3-Clause): facts, no code

No MAME code is in this crate. Three facts it carries (through the reference) are as MAME's
BSD-3-Clause files model them, and are credited here with those files' copyright lines:

- `src/syscard.rs`: the Super System Card id registers `$18C1-$18C7` (eight bytes), as in MAME
  `src/devices/bus/pce/pce_scdsys.cpp` (copyright-holders: Fabio Priuli, Wilbert Pol);
- `src/adpcm.rs`: the MSM5205 step formula (`DELTA`), as modelled by MAME
  `src/devices/sound/msm5205.cpp` (copyright-holders: Aaron Giles);
- `src/scsi.rs` (the clean rewrite of 2026-11-19, transcribed from pcenginecd-python `4abb3f5`):
  the vendor commands' 10-byte blocks, READ TOC's answer lengths and track `$AA`, the auto-ACK
  pulse of 15 CPU cycles, a READ of zero sectors and PAUSE without audio, as in MAME
  `src/mame/nec/pce_cd.cpp` (copyright-holders: Wilbert Pol, Angelo Salese); facts only, as the
  reference records (with ares, ISC, for the same facts).

The licence of those files:

```
BSD 3-Clause License

Copyright (c) the copyright holders named above

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its
   contributors may be used to endorse or promote products derived from
   this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```
