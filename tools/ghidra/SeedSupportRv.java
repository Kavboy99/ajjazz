// AJAZZ support ID1759 x1. Offline analysis only.
// @category AJAZZ
import ghidra.app.script.GhidraScript;
import ghidra.program.model.mem.MemoryBlock;
import java.math.BigInteger;
public class SeedSupportRv extends GhidraScript {
    public void run() throws Exception {
        MemoryBlock ram = currentProgram.getMemory().getBlock(toAddr(0x20000000L));
        if (ram == null) ram = currentProgram.getMemory().createUninitializedBlock(
            "RAM", toAddr(0x20000000L), 0x10000, false);
        ram.setWrite(true);
        if (!ram.isInitialized()) ram = currentProgram.getMemory().convertToInitialized(ram, (byte)0);
        currentProgram.getMemory().setBytes(toAddr(0x20000000L), getBytes(toAddr(0x7bc8), 0xc8));
        currentProgram.getProgramContext().setValue(currentProgram.getRegister("gp"),
            toAddr(0), toAddr(0x4fff), BigInteger.valueOf(0x20000858L));
        currentProgram.getProgramContext().setValue(currentProgram.getRegister("gp"),
            toAddr(0x5000), toAddr(0x7c8f), BigInteger.valueOf(0x200008b8L));
        MemoryBlock mmio = currentProgram.getMemory().getBlock(toAddr(0x40000000L));
        if (mmio == null) mmio = currentProgram.getMemory().createUninitializedBlock(
            "MMIO", toAddr(0x40000000L), 0x300000, false);
        mmio.setWrite(true); mmio.setVolatile(true);
        for (long entry : new long[]{0, 0x14f2, 0x11f0, 0x5000, 0x707c, 0x6dac}) {
            disassemble(toAddr(entry));
            if (getFunctionAt(toAddr(entry)) == null) createFunction(toAddr(entry), null);
            currentProgram.getSymbolTable().addExternalEntryPoint(toAddr(entry));
        }
        for (long at = 0x5038; at < 0x5200; at += 4) {
            long target = Integer.toUnsignedLong(getInt(toAddr(at)));
            if (target < 0x5200 || target >= 0x7bc8 || (target & 1) != 0) continue;
            disassemble(toAddr(target));
            if (getFunctionAt(toAddr(target)) == null) createFunction(toAddr(target), null);
            currentProgram.getSymbolTable().addExternalEntryPoint(toAddr(target));
        }
    }
}
