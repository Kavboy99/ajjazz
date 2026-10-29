// Pinned AK820MAX 80A0 support image: low mapped user ROM, Cortex-M4 Thumb.
// @category AJAZZ
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.mem.MemoryBlock;
import java.math.BigInteger;

public class SeedKeyboard136 extends GhidraScript {
    @Override public void run() throws Exception {
        MemoryBlock rom = currentProgram.getMemory().getBlock(toAddr(0));
        if (rom == null || rom.getSize() != 0x7e000) throw new IllegalStateException("Unexpected user ROM");
        rom.setExecute(true);
        currentProgram.getProgramContext().setValue(currentProgram.getRegister("TMode"),
            toAddr(0), toAddr(0x7dfff), BigInteger.ONE);
        if(currentProgram.getMemory().getBlock(toAddr(0x20000000L))==null) {
            MemoryBlock ram=currentProgram.getMemory().createUninitializedBlock("SRAM",toAddr(0x20000000L),0x10000,false);
            ram.setRead(true);ram.setWrite(true);
        }
        for(long[] range:new long[][]{{0x40000000L,0x100000},{0xe000e000L,0x2000}}) {
            if(currentProgram.getMemory().getBlock(toAddr(range[0]))==null) {
                MemoryBlock mmio=currentProgram.getMemory().createUninitializedBlock("MMIO_"+Long.toHexString(range[0]),toAddr(range[0]),range[1],false);
                mmio.setRead(true);mmio.setWrite(true);
            }
        }
        for(int offset=4;offset<0x200;offset+=4) {
            long pointer=Integer.toUnsignedLong(getInt(toAddr(offset)));
            long target=pointer&~1L;
            if((pointer&1)==0 || target<0x200 || target>=0x7e000)continue;
            Address at=toAddr(target);disassemble(at);
            if(getFunctionAt(at)==null)createFunction(at,null);
            currentProgram.getSymbolTable().addExternalEntryPoint(at);
        }
        println("Seeded AK820MAX support image vectors and memory map");
    }
}
