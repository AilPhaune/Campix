use alloc::vec::Vec;

use crate::io::{inl, outl};

const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

/// Represents a detected PCI device
#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub os_class_name: &'static str,
}

pub fn get_class_name(class: u8, subclass: u8, prog_if: u8) -> &'static str {
    match (class, subclass, prog_if) {
        (0x00, 0x00, _) => "Non-VGA-Compatible Unclassified Device",
        (0x00, 0x01, _) => "VGA-Compatible Unclassified Device",
        (0x01, 0x01, 0x0) => {
            "Mass Storage Controller: IDE Controller: ISA Compatibility mode-only controller"
        }
        (0x01, 0x01, 0x5) => {
            "Mass Storage Controller: IDE Controller: PCI native mode-only controller "
        }
        (0x01, 0x01, 0xA) => {
            "Mass Storage Controller: IDE Controller: ISA Compatibility mode controller, supports both channels switched to PCI native mode"
        }
        (0x01, 0x01, 0xF) => {
            "Mass Storage Controller: IDE Controller: PCI native mode controller, supports both channels switched to ISA compatibility mode"
        }
        (0x01, 0x01, 0x80) => {
            "Mass Storage Controller: IDE Controller: ISA Compatibility mode-only controller, supports bus mastering"
        }
        (0x01, 0x01, 0x85) => {
            "Mass Storage Controller: IDE Controller: PCI native mode-only controller, supports bus mastering"
        }
        (0x01, 0x01, 0x8A) => {
            "Mass Storage Controller: IDE Controller: ISA Compatibility mode controller, supports both channels switched to PCI native mode, supports bus mastering"
        }
        (0x01, 0x01, 0x8F) => {
            "Mass Storage Controller: IDE Controller: PCI native mode controller, supports both channels switched to ISA compatibility mode, supports bus mastering"
        }
        (0x01, 0x01, _) => "Mass Storage Controller: IDE Controller: Unknown",
        (0x01, 0x02, _) => "Mass Storage Controller: Floppy Disk Controller",
        (0x01, 0x03, _) => "Mass Storage Controller: IPI Controller",
        (0x01, 0x04, _) => "Mass Storage Controller: RAID Controller",
        (0x01, 0x05, 0x20) => "Mass Storage Controller: ATA Controller: Single DMA",
        (0x01, 0x05, 0x30) => "Mass Storage Controller: ATA Controller: Chained DMA",
        (0x01, 0x05, _) => "Mass Storage Controller: ATA Controller: Unknown",
        (0x01, 0x06, 0x00) => "Mass Storage Controller: Serial ATA Controller: Vendor Specific Interface",
        (0x01, 0x06, 0x01) => "Mass Storage Controller: Serial ATA Controller: AHCI 1.0",
        (0x01, 0x06, 0x02) => "Mass Storage Controller: Serial ATA Controller: Serial Storage Bus",
        (0x01, 0x06, _) => "Mass Storage Controller: Serial ATA Controller: Unknown",
        (0x01, 0x07, 0x00) => "Mass Storage Controller: Serial Attached SCSI Controller: SAS",
        (0x01, 0x07, 0x01) => "Mass Storage Controller: Serial Attached SCSI Controller: Serial Storage Bus",
        (0x01, 0x07, _) => "Mass Storage Controller: Serial Attached SCSI Controller: Unknown",
        (0x01, 0x08, 0x01) => "Mass Storage Controller: Non-Volatile Memory Controller: NVMHCI",
        (0x01, 0x08, 0x02) => "Mass Storage Controller: Non-Volatile Memory Controller: NVM Express",
        (0x01, 0x08, _) => "Mass Storage Controller: Non-Volatile Memory Controller: Unknown",
        (0x01, 0x80, _) => "Mass Storage Controller: Other",
        (0x01, _, _) => "Mass Storage Controller: Unknown",
        (0x02, 0x00, _) => "Network Controller: Ethernet Controller",
        (0x02, 0x01, _) => "Network Controller: Token Ring Controller",
        (0x02, 0x02, _) => "Network Controller: FDDI Controller",
        (0x02, 0x03, _) => "Network Controller: ATM Controller",
        (0x02, 0x04, _) => "Network Controller: ISDN Controller",
        (0x02, 0x05, _) => "Network Controller: WorldFip Controller",
        (0x02, 0x06, _) => "Network Controller: PICMG 2.14 Multi Computing Controller",
        (0x02, 0x07, _) => "Network Controller: Infiniband Controller",
        (0x02, 0x08, _) => "Network Controller: Fabric Controller",
        (0x02, 0x80, _) => "Network Controller: Other",
        (0x02, _, _) => "Network Controller: Unknown",
        (0x03, 0x00, 0x00) => "Display Controller: VGA Compatible Controller: VGA Controller",
        (0x03, 0x00, 0x01) => "Display Controller: VGA Compatible Controller: 8514-Compatible Controller",
        (0x03, 0x00, _) => "Display Controller: VGA Compatible Controller: Unknown",
        (0x03, 0x01, _) => "Display Controller: XGA Controller",
        (0x03, 0x02, _) => "Display Controller: 3D Controller (Not VGA-Compatible)",
        (0x03, 0x80, _) => "Display Controller: Other",
        (0x03, _, _) => "Display Controller: Unknown",
        (0x04, 0x00, _) => "Multimedia Controller: Multimedia Video Controller",
        (0x04, 0x01, _) => "Multimedia Controller: Multimedia Audio Controller",
        (0x04, 0x02, _) => "Multimedia Controller: Computer Telephony Controller",
        (0x04, 0x03, _) => "Multimedia Controller: Audio Device",
        (0x04, 0x80, _) => "Multimedia Controller: Other",
        (0x04, _, _) => "Multimedia Controller: Unknown",
        (0x05, 0x00, _) => "Memory Controller: RAM Controller",
        (0x05, 0x01, _) => "Memory Controller: Flash Controller",
        (0x05, 0x80, _) => "Memory Controller: Other",
        (0x05, _, _) => "Memory Controller: Unknown",
        (0x06, 0x00, _) => "Bridge Device: Host Bridge",
        (0x06, 0x01, _) => "Bridge Device: ISA Bridge",
        (0x06, 0x02, _) => "Bridge Device: EISA Bridge",
        (0x06, 0x03, _) => "Bridge Device: MCA Bridge",
        (0x06, 0x04, 0x00) => "Bridge Device: PCI-to-PCI Bridge: Normal Decode",
        (0x06, 0x04, 0x01) => "Bridge Device: PCI-to-PCI Bridge: Subtractive Decode",
        (0x06, 0x04, _) => "Bridge Device: PCI-to-PCI Bridge: Unknown",
        (0x06, 0x05, _) => "Bridge Device: PCMCIA Bridge",
        (0x06, 0x06, _) => "Bridge Device: NuBus Bridge",
        (0x06, 0x07, _) => "Bridge Device: CardBus Bridge",
        (0x06, 0x08, 0x00) => "Bridge Device: RACEway Bridge: Transparent Mode",
        (0x06, 0x08, 0x01) => "Bridge Device: RACEway Bridge: Endpoint Mode",
        (0x06, 0x08, _) => "Bridge Device: RACEway Bridge: Unknown",
        (0x06, 0x09, 0x40) => "Bridge Device: PCI-to-PCI Bridge: Semi-Transparent, Primary bus towards host CPU",
        (0x06, 0x09, 0x80) => "Bridge Device: PCI-to-PCI Bridge: Semi-Transparent, Secondary bus towards host CPU",
        (0x06, 0x09, _) => "Bridge Device: PCI-to-PCI Bridge: Unknown",
        (0x06, 0x0A, _) => "Bridge Device: InfiniBand-to-PCI Host Bridge",
        (0x06, 0x80, _) => "Bridge Device: Other",
        (0x06, _, _) => "Bridge Device: Unknown",
        (0x07, 0x00, 0x00) => "Simple Communication Controller: Serial Controller: 8250-Compatible (Generic XT)",
        (0x07, 0x00, 0x01) => "Simple Communication Controller: Serial Controller: 16450-Compatible",
        (0x07, 0x00, 0x02) => "Simple Communication Controller: Serial Controller: 16550-Compatible",
        (0x07, 0x00, 0x03) => "Simple Communication Controller: Serial Controller: 16650-Compatible",
        (0x07, 0x00, 0x04) => "Simple Communication Controller: Serial Controller: 16750-Compatible",
        (0x07, 0x00, 0x05) => "Simple Communication Controller: Serial Controller: 16850-Compatible",
        (0x07, 0x00, 0x06) => "Simple Communication Controller: Serial Controller: 16950-Compatible",
        (0x07, 0x00, _) => "Simple Communication Controller: Serial Controller: Unknown",
        (0x07, 0x01, 0x00) => "Simple Communication Controller: Parallel Controller: Standard Parallel Port",
        (0x07, 0x01, 0x01) => "Simple Communication Controller: Parallel Controller: Bi-Directional Parallel Port",
        (0x07, 0x01, 0x02) => "Simple Communication Controller: Parallel Controller: ECP 1.X Compliant Parallel Port ",
        (0x07, 0x01, 0x03) => "Simple Communication Controller: Parallel Controller: IEEE 1284 Controller",
        (0x07, 0x01, 0xFE) => "Simple Communication Controller: Parallel Controller: IEEE 1284 Target Device",
        (0x07, 0x01, _) => "Simple Communication Controller: Parallel Controller: Unknown",
        (0x07, 0x02, _) => "Simple Communication Controller: Multiport Serial Controller",
        (0x07, 0x03, 0x00) => "Simple Communication Controller: Modem: Generic Modem",
        (0x07, 0x03, 0x01) => "Simple Communication Controller: Modem: Hayes 16450-Compatible Interface",
        (0x07, 0x03, 0x02) => "Simple Communication Controller: Modem: Hayes 16550-Compatible Interface",
        (0x07, 0x03, 0x03) => "Simple Communication Controller: Modem: Hayes 16650-Compatible Interface",
        (0x07, 0x03, 0x04) => "Simple Communication Controller: Modem: Hayes 16750-Compatible Interface",
        (0x07, 0x03, _) => "Simple Communication Controller: Modem: Unknown",
        (0x07, 0x04, _) => "Simple Communication Controller: IEEE 488.1/2 (GPIB) Controller",
        (0x07, 0x05, _) => "Simple Communication Controller: Smart Card Controller",
        (0x07, 0x06, _) => "Simple Communication Controller: Other",
        (0x07, _, _) => "Simple Communication Controller: Unknown",
        (0x08, 0x00, 0x00) => "Base System Peripheral: PIC: Generic 8259-Compatible",
        (0x08, 0x00, 0x01) => "Base System Peripheral: PIC: ISA-Compatible",
        (0x08, 0x00, 0x02) => "Base System Peripheral: PIC: EISA-Compatible",
        (0x08, 0x00, 0x10) => "Base System Peripheral: PIC: I/O APIC Interrupt Controller ",
        (0x08, 0x00, 0x20) => "Base System Peripheral: PIC: I/O(x) APIC Interrupt Controller",
        (0x08, 0x00, _) => "Base System Peripheral: PIC: Unknown",
        (0x08, 0x01, 0x00) => "Base System Peripheral: DMA Controller: Generic 8237-Compatible",
        (0x08, 0x01, 0x01) => "Base System Peripheral: DMA Controller: ISA-Compatible ",
        (0x08, 0x01, 0x02) => "Base System Peripheral: DMA Controller: EISA-Compatible",
        (0x08, 0x01, _) => "Base System Peripheral: DMA Controller: Unknown",
        (0x08, 0x02, 0x00) => "Base System Peripheral: Timer: Generic 8254-Compatible",
        (0x08, 0x02, 0x01) => "Base System Peripheral: Timer: ISA-Compatible",
        (0x08, 0x02, 0x02) => "Base System Peripheral: Timer: EISA-Compatible",
        (0x08, 0x02, 0x03) => "Base System Peripheral: Timer: HPET",
        (0x08, 0x02, _) => "Base System Peripheral: Timer: Unknown",
        (0x08, 0x03, 0x00) => "Base System Peripheral: RCT Controller: Generic RTC",
        (0x08, 0x03, 0x01) => "Base System Peripheral: RCT Controller: ISA-Compatible",
        (0x08, 0x03, _) => "Base System Peripheral: RCT Controller: Unknown",
        (0x08, 0x04, _) => "Base System Peripheral: PCI Hot-Plug Controller",
        (0x08, 0x05, _) => "Base System Peripheral: SD Host controller",
        (0x08, 0x06, _) => "Base System Peripheral: IOMMU",
        (0x08, 0x80, _) => "Base System Peripheral: Other",
        (0x08, _, _) => "Base System Peripheral: Unknown",
        (0x09, 0x00, _) => "Input Device Controller: Keyboard Controller",
        (0x09, 0x01, _) => "Input Device Controller: Digitizer Pen",
        (0x09, 0x02, _) => "Input Device Controller: Mouse Controller",
        (0x09, 0x03, _) => "Input Device Controller: Scanner Controller",
        (0x09, 0x04, 0x00) => "Input Device Controller: Gameport Controller: Generic",
        (0x09, 0x04, 0x01) => "Input Device Controller: Gameport Controller: Extended",
        (0x09, 0x04, _) => "Input Device Controller: Gameport Controller: Unknown",
        (0x09, 0x80, _) => "Input Device Controller: Other",
        (0x09, _, _) => "Input Device Controller: Unknown",
        (0x0A, 0x00, _) => "Docking Station: Generic",
        (0x0A, 0x80, _) => "Docking Station: Other",
        (0x0A, _, _) => "Docking Station: Unknown",
        (0x0B, 0x00, _) => "Processor: 386",
        (0x0B, 0x01, _) => "Processor: 486",
        (0x0B, 0x02, _) => "Processor: Pentium",
        (0x0B, 0x03, _) => "Processor: Pentium Pro",
        (0x0B, 0x10, _) => "Processor: Alpha",
        (0x0B, 0x20, _) => "Processor: PowerPC",
        (0x0B, 0x30, _) => "Processor: MIPS",
        (0x0B, 0x40, _) => "Processor: Co-processor",
        (0x0B, 0x80, _) => "Processor: Other",
        (0x0B, _, _) => "Processor: Unknown",
        (0x0C, 0x00, 0x00) => "Serial Bus Controller: FireWire (IEEE 1394) Controller: Generic",
        (0x0C, 0x00, 0x10) => "Serial Bus Controller: FireWire (IEEE 1394) Controller: OHCI",
        (0x0C, 0x00, _) => "Serial Bus Controller: FireWire (IEEE 1394) Controller: Unknown",
        (0x0C, 0x01, _) => "Serial Bus Controller: ACCESS Bus Controller",
        (0x0C, 0x02, _) => "Serial Bus Controller: SSA",
        (0x0C, 0x03, 0x00) => "Serial Bus Controller: USB Controller: UHCI Controller",
        (0x0C, 0x03, 0x10) => "Serial Bus Controller: USB Controller: OHCI Controller",
        (0x0C, 0x03, 0x20) => "Serial Bus Controller: USB Controller: EHCI (USB2) Controller",
        (0x0C, 0x03, 0x30) => "Serial Bus Controller: USB Controller: XHCI (USB3) Controller",
        (0x0C, 0x03, 0x80) => "Serial Bus Controller: USB Controller: Unspecified",
        (0x0C, 0x03, 0xFE) => "Serial Bus Controller: USB Controller: USB Device (Not a host controller)",
        (0x0C, 0x03, _) => "Serial Bus Controller: USB Controller: Unknown",
        (0x0C, 0x04, _) => "Serial Bus Controller: Fibre Channel",
        (0x0C, 0x05, _) => "Serial Bus Controller: SMBus Controller",
        (0x0C, 0x06, _) => "Serial Bus Controller: InfiniBand Controller",
        (0x0C, 0x07, 0x00) => "Serial Bus Controller: IPMI Interface: SMIC",
        (0x0C, 0x07, 0x01) => "Serial Bus Controller: IPMI Interface: Keyboard Controller Style",
        (0x0C, 0x07, 0x02) => "Serial Bus Controller: IPMI Interface: Block Transfer",
        (0x0C, 0x07, _) => "Serial Bus Controller: IPMI Interface: Unknown",
        (0x0C, 0x08, _) => "Serial Bus Controller: SERCOS Interface (IEC 61491)",
        (0x0C, 0x09, _) => "Serial Bus Controller: CANbus Controller",
        (0x0C, 0x80, _) => "Serial Bus Controller: Other",
        (0x0C, _, _) => "Serial Bus Controller: Unknown",
        (0x0D, 0x00, _) => "Wireless Controller: iRDA Compatible Controller",
        (0x0D, 0x01, _) => "Wireless Controller: Consumer IR Controller",
        (0x0D, 0x10, _) => "Wireless Controller: RF Controller",
        (0x0D, 0x11, _) => "Wireless Controller: Bluetooth Controller",
        (0x0D, 0x12, _) => "Wireless Controller: Broadband Controller",
        (0x0D, 0x20, _) => "Wireless Controller: Ethernet Controller (802.1a)",
        (0x0D, 0x21, _) => "Wireless Controller: Ethernet Controller (802.1b)",
        (0x0D, 0x80, _) => "Wireless Controller: Other",
        (0x0D, _, _) => "Wireless Controller: Unknown",
        (0x0E, 0x00, _) => "Intelligent Controller: I20",
        (0x0E, _, _) => "Intelligent Controller: Unknown",
        (0x0F, 0x01, _) => "Satellite Communication Controller: Satellite TV Controller",
        (0x0F, 0x02, _) => "Satellite Communication Controller: Satellite Audio Controller",
        (0x0F, 0x03, _) => "Satellite Communication Controller: Satellite Voice Controller",
        (0x0F, 0x04, _) => "Satellite Communication Controller: Satellite Data Controller",
        (0x0F, _, _) => "Satellite Communication Controller: Unknown",
        (0x10, 0x00, _) => "Encryption Controller: Network and Computing Encrpytion/Decryption",
        (0x10, 0x10, _) => "Encryption Controller: Entertainment Encryption/Decryption",
        (0x10, 0x80, _) => "Encryption Controller: Other",
        (0x10, _, _) => "Encryption Controller: Unknown",
        (0x11, 0x00, _) => "Signal Processing Controller: DPIO Modules",
        (0x11, 0x01, _) => "Signal Processing Controller: Performance Counters",
        (0x11, 0x10, _) => "Signal Processing Controller: Communication Synchronizer",
        (0x11, 0x20, _) => "Signal Processing Controller: Signal Processing Management",
        (0x11, 0x80, _) => "Signal Processing Controller: Other",
        (0x11, _, _) => "Signal Processing Controller: Unknown",
        (0x12, _, _) => "Processing Accelerator",
        (0x13, _, _) => "Non-Essential Instrumentation",
        (0x14, _, _) => "0x3F (Reserved)",
        (0x40, _, _) => "Co-processor",
        (0x41, _, _) => "0xFE (Reserved)",
        _ => "Unknown",
    }
}

/// Reads a 32-bit config register from a PCI device
unsafe fn read_config(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = (1u32 << 31)
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC);
    outl(PCI_CONFIG_ADDRESS, address);
    inl(PCI_CONFIG_DATA)
}

/// Scans the entire PCI bus and returns all devices
pub fn scan_bus() -> Vec<PciDevice> {
    let mut devices = Vec::new();

    for bus in 0u8..=255 {
        for device in 0u8..32 {
            for function in 0u8..8 {
                let vendor_device = unsafe { read_config(bus, device, function, 0x00) };
                let vendor_id = (vendor_device & 0xFFFF) as u16;
                if vendor_id == 0xFFFF {
                    continue;
                }

                let device_id = ((vendor_device >> 16) & 0xFFFF) as u16;
                let class_subclass = unsafe { read_config(bus, device, function, 0x08) };
                let class = ((class_subclass >> 24) & 0xFF) as u8;
                let subclass = ((class_subclass >> 16) & 0xFF) as u8;
                let prog_if = ((class_subclass >> 8) & 0xFF) as u8;

                let device_info = PciDevice {
                    bus,
                    device,
                    function,
                    vendor_id,
                    device_id,
                    class,
                    subclass,
                    prog_if,
                    os_class_name: get_class_name(class, subclass, prog_if),
                };

                devices.push(device_info);
            }
        }
    }

    devices
}
