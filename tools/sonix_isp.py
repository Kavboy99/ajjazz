"""Recovered Sonix ISP packet codec and offline model; no hardware transport.

Payload offsets exclude the Windows report ID. These factories describe the
SN34F280 path in the pinned OEM updater, not every Sonix bootloader.
"""
from __future__ import annotations
import struct
from keyboard_updater_lab import require, word_sum

PAYLOAD_SIZE = 64
ACK = b'\xfa' * 4
SN34F280 = dict(name='SN34F280',boot_vid=0x0c45,boot_pid=0x7210,
    signature='20080000',image_bytes=0x7e000,erase_pages=504,
    program_blocks=8064,blank_checksum=0,code_security_0=0,
    evidence='OEM 0x5B34D7, descriptor 0x5B3060; constructors 0x40B6A0..0x40C140')


def command(number, size=64, subcommand=0):
    require(number in range(1,10) and size in (8,64), 'Unsupported command/size')
    data=bytearray(size)
    data[:4]=bytes([number,0xaa,0x55,subcommand])
    return data


def get_version():
    return bytes(command(1))


def compare_option():
    # The bulk-download path calls 0x40BAC0 with a zeroed CS0 context.
    return bytes(command(2))


def set_option(option=0, security=0):
    require(option==0 and security==0,'Only captured CS0 settings are modelled')
    data=command(3)
    struct.pack_into('<HH',data,4,option,security)
    return bytes(data)


def erase(start_page=0, end_page=504):
    require(start_page==0 and end_page==504,'Only pinned SN34F280 erase geometry is verified')
    data=command(4)
    struct.pack_into('<H',data,4,start_page)
    struct.pack_into('<H',data,8,end_page)
    return bytes(data)


def enable_program(blocks=8064):
    require(blocks==8064,'Only full SN34F280 download is verified')
    data=command(5)
    struct.pack_into('<H',data,8,blocks)
    return bytes(data)


def get_checksum():
    return bytes(command(6,8))


def return_user():
    return bytes(command(7,8))


def windows_report(payload, report_bytes=65):
    require(report_bytes==65 and len(payload)<=64,'Expected 65-byte Feature report')
    return b'\0'+payload.ljust(64,b'\0')


def check_reply(reply, request, *, ack=False):
    require(len(reply)==64,'Truncated/unexpected ISP response length')
    require(reply[:4]==request[:4],'ISP response header mismatch')
    if ack:
        require(reply[4:8]==ACK,'ISP response ACK mismatch')
    return reply


def decode_version(reply):
    check_reply(reply,get_version())
    require(reply[8:12]==bytes.fromhex(SN34F280['signature']), 'Unknown chip/version signature')
    security=struct.unpack_from('<H',reply,14)[0]
    require(security==0,'Only observed OEM CS0 path is modelled')
    return dict(chip='SN34F280',signature=reply[8:12].hex(),security_raw=security)


class DownloadModel:
    """Deterministic transport-free download transcript for integration tests.

    This deliberately has no retry of an unacknowledged erase/data operation and
    no code-security transitions. A hardware engine requires device identity,
    re-enumeration, acknowledgement and recovery validation in addition to this.
    """
    def __init__(self, image):
        require(len(image)==SN34F280['image_bytes'],'Wrong image length')
        self.image=bytes(image)
        self.state='version'
        self.offset=0
        self.history=[]

    def next_payload(self):
        requests={'version':get_version,'compare':compare_option,'option':set_option,
                  'erase':erase,'program':enable_program,'reboot':return_user}
        if self.state=='data':
            return self.image[self.offset:self.offset+64]
        if self.state=='final_reply':
            return None
        require(self.state in requests,'Download already complete/failed')
        return requests[self.state]()

    def acknowledge(self, reply=None):
        require(self.state not in ('complete','failed'),'Download already complete/failed')
        old=self.state
        try:
            if old=='version':
                decode_version(reply)
                self.state='compare'
            elif old in ('compare','option'):
                # OEM only checks SetFeature success in these two constructors.
                require(reply is None,'These are send-completion transitions')
                self.state='option' if old=='compare' else 'erase'
            elif old=='erase':
                require(len(reply)==64 and struct.unpack_from('<H',reply,8)[0]==0,
                        'Blank checksum mismatch')
                self.state='program'
            elif old=='program':
                check_reply(reply,enable_program(),ack=True)
                self.state='data'
            elif old=='data':
                require(reply is None,'Data progress is successful transfer completion')
                self.offset+=64
                if self.offset==len(self.image):
                    self.state='final_reply'
            elif old=='final_reply':
                # 0x407CA0 performs GetFeature after the raw data stream. It
                # compares the checksum at +8, without requiring a header/ACK.
                require(len(reply)==64 and struct.unpack_from('<H',reply,8)[0]==word_sum(self.image),
                        'Final image checksum mismatch')
                self.state='reboot'
            elif old=='reboot':
                require(reply is None,'Reboot is a send-completion transition')
                self.state='complete'
        except (ValueError,TypeError):
            self.state='failed'
            raise
        self.history.append((old,self.state))


def simulated_transcript(image):
    model=DownloadModel(image)
    version=bytearray(get_version());version[8:12]=bytes.fromhex(SN34F280['signature'])
    model.acknowledge(bytes(version));model.acknowledge();model.acknowledge()
    model.acknowledge(bytes(64))
    model.acknowledge(enable_program()[:4]+ACK+bytes(56))
    data=bytearray()
    while model.state=='data':
        data.extend(model.next_payload());model.acknowledge()
    require(data==image,'Transcript data mismatch')
    final=bytearray(64);struct.pack_into('<H',final,8,word_sum(image))
    model.acknowledge(bytes(final));model.acknowledge()
    return dict(state=model.state,data_blocks=len(data)//64,image_bytes=len(data),
        word_checksum=f'{word_sum(data):04X}',hardware_tested=False)
