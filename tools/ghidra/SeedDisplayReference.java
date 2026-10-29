// Reference Keydous/EPOMAKER display MCU, not an AJ159 replacement image.
// @category AJAZZ
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.mem.MemoryBlock;
import java.math.BigInteger;

public class SeedDisplayReference extends GhidraScript {
    @Override public void run() throws Exception {
        String[] args = getScriptArgs();
        boolean epomaker = args.length == 1 && args[0].equals("epomaker");
        if (args.length > 1 || (args.length == 1 && !epomaker && !args[0].equals("keydous")))
            throw new IllegalArgumentException("Expected keydous or epomaker");
        final long base = 0x01000200L;
        MemoryBlock flash = currentProgram.getMemory().getBlock(toAddr(base));
        boolean fresh = flash == null;
        if (flash == null) {
            flash = currentProgram.getMemory().getBlock(toAddr(0));
            currentProgram.getMemory().moveBlock(flash, toAddr(base), monitor);
        }
        flash.setExecute(true);
        if (fresh) currentProgram.getProgramContext().setValue(currentProgram.getRegister("TMode"),
            toAddr(base), toAddr(base + flash.getSize() - 1), BigInteger.ONE);
        MemoryBlock ram = currentProgram.getMemory().getBlock(toAddr(0x20000L));
        if (ram == null) ram = currentProgram.getMemory().createUninitializedBlock(
            "reference_RAM", toAddr(0x20000L), 0x10000, false);
        ram.setRead(true); ram.setWrite(true);
        // File offsets, converted using the YC3121 Keil/IAR flash origin.
        // Keydous has a16-byte tag; EPOMAKER begins with executable thunks.
        long[] seeds = epomaker ? new long[]{0, 0x200, 0x10000, 0x10200} :
                                     new long[]{0, 0x200, 0x876, 0xa3c, 0xd12, 0xdce,
                                      0x10010, 0x10200, 0x148ca, 0x149da,
                                      0x1460c, 0x14476};
        for (long offset : seeds) {
            Address target = toAddr(base + offset);
            disassemble(target);
            if (getFunctionAt(target) == null) createFunction(target, null);
            currentProgram.getSymbolTable().addExternalEntryPoint(target);
        }
        if (epomaker) {
            for (long origin : new long[]{base, base + 0x10000}) {
                for (long offset = 0; offset < 0x180; offset += 2) {
                    Address at = toAddr(origin + offset);
                    int instruction = getShort(at) & 0xffff;
                    if ((instruction & 0xff00) != 0x4800 || (getShort(at.add(2)) & 0xffff) != 0x4700) continue;
                    long cell = ((origin + offset + 4) & ~3L) + (instruction & 255) * 4;
                    long pointer = Integer.toUnsignedLong(getInt(toAddr(cell)));
                    long targetValue = pointer & ~1L;
                    if ((pointer & 1) == 0 || targetValue < base || targetValue >= base + flash.getSize()) continue;
                    Address target = toAddr(targetValue);
                    disassemble(target);
                    if (getFunctionAt(target) == null) createFunction(target, null);
                    currentProgram.getSymbolTable().addExternalEntryPoint(target);
                }
            }
        }
        println("Seeded display reference at 01000200; target dock compatibility unknown.");
    }
}
