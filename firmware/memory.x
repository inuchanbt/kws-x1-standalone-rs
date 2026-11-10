MEMORY
{
  /* Executable application space ends at the container descriptor at 0x433fc,
   * before the checksum at 0x4340c. Packaging generates zero fill and footer
   * metadata; the payload ends at 0x43640 and its erase envelope at 0x44000.
   * Bootloader below 0x10000 and settings at [0x72000, 0x80000) are untouched.
   */
  FLASH : ORIGIN = 0x00010000, LENGTH = 0x333fc
  /* Upper code region [0x44000, 0x62000): the release image's unpacker
   * programs it. The next six sectors are for screenshots. The sector at
   * 0x6e000 holds factory data that must never be erased.
   * Matches firmware_image::UPPER_BASE/UPPER_END.
   */
  UPPER : ORIGIN = 0x00044000, LENGTH = 0x1e000
  /* SRAMH (32 KiB) upper half + SRAM1 (64 KiB). The origin leaves the original
   * firmware's clock bookkeeping words at 0x1FFF82A4/0x1FFF8C54 outside this image's
   * .bss, and the length stops 256 bytes short of SRAM1's exclusive end so the
   * initial stack pointer is strictly inside SRAM1. The 64.8 KiB frame buffer
   * plus stack fit comfortably; SRAM3 is left untouched.
   */
  RAM : ORIGIN = 0x1FFF9000, LENGTH = 0x15F00
  /* SRAM2 (64 KiB) holds the 64.8 KiB frame buffer, leaving room for the PD
   * stack in RAM. SRAM1 and SRAM2 share the SRAM12 wait-state field
   * (SRAM_WTCR), so their timing is the same. SRAM3 (ECC errata) stays unused.
   */
  SRAM2 : ORIGIN = 0x20010000, LENGTH = 0x10000
}

SECTIONS
{
  .sram2 (NOLOAD) : ALIGN(4)
  {
    *(.sram2 .sram2.*);
  } > SRAM2
} INSERT AFTER .uninit;

/* cortex-m-rt takes these from the location counter after .uninit, which is
 * now in SRAM2: keep the stack (and the unused heap start) in RAM. */
_stack_end = __euninit;
__sheap = __euninit;
/* Keep close to the v0.12.0 stack budget (~45 KiB) when adding static buffers. */
ASSERT(_stack_start - _stack_end >= 0xa000, "less than 40 KiB of stack remains");

/* The PD stack, protocol crates and FFT tables (selected by their v0
 * crate/module roots in per-symbol section names) run from the upper region.
 * Inserted before .text so these patterns claim the sections first. Nothing
 * may call into it until startup has checked the installed region (`upper.rs`).
 */
SECTIONS
{
  .upper : ALIGN(4)
  {
    __supper = .;
    *(.text.capture_reclaim_hash .rodata.capture_reclaim_hash);
    *(.text.meter_header);
    *(.text.*_5usbpd* .text.*_12usbpd_traits* .text.*_11pd_protocol* .text.*_10fusb302_pd* .text.*_8firmware7pd_task* .text.*_8microfft* .text.*_21single_wire_protocols*);
    *(.rodata.*_5usbpd* .rodata.*_12usbpd_traits* .rodata.*_11pd_protocol* .rodata.*_10fusb302_pd* .rodata.*_8firmware7pd_task* .rodata.*_8microfft* .rodata.*_21single_wire_protocols*);
    . = ALIGN(4);
    __eupper = .;
  } > UPPER
} INSERT BEFORE .text;
