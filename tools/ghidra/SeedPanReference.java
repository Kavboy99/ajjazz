// Seed reference-only Cortex-M analysis from observed vector tables.
// No hardware access, vendor execution or binary patching.
// @category AJAZZ
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.SourceType;
import java.math.BigInteger;

public class SeedPanReference extends GhidraScript {
    @Override public void run() throws Exception {
        Address first = currentProgram.getMinAddress();
        Address last = currentProgram.getMaxAddress();
        currentProgram.getProgramContext().setValue(
            currentProgram.getRegister("TMode"), first, last, BigInteger.ONE);
        MemoryBlock ram = currentProgram.getMemory().createUninitializedBlock(
            "RAM_reference", toAddr(0x20000000L), 0x10000, false);
        ram.setRead(true); ram.setWrite(true); ram.setExecute(true);
        MemoryBlock peripherals = currentProgram.getMemory().createUninitializedBlock(
            "MMIO_reference", toAddr(0x40000000L), 0x200000, false);
        peripherals.setRead(true); peripherals.setWrite(true); peripherals.setVolatile(true);
        MemoryBlock cortex = currentProgram.getMemory().createUninitializedBlock(
            "Cortex_MMIO", toAddr(0xe000e000L), 0x2000, false);
        cortex.setRead(true); cortex.setWrite(true); cortex.setVolatile(true);
        for (long vectors : new long[]{0, 0x30200}) {
            for (int index = 1; index < 48; index++) {
                long pointer = Integer.toUnsignedLong(getInt(toAddr(vectors + 4 * index)));
                long address = pointer & ~1L;
                if ((pointer & 1) == 0 || address < 0x80 || address > last.getOffset()) continue;
                Address target = toAddr(address);
                disassemble(target);
                if (getFunctionAt(target) == null) createFunction(target, null);
                currentProgram.getSymbolTable().addExternalEntryPoint(target);
            }
        }
        if (currentProgram.getName().startsWith("mouse-")) {
            Address dispatcher = toAddr(0xcec);
            disassemble(dispatcher);
            if (getFunctionAt(dispatcher) == null) createFunction(dispatcher, "reference_boot_dispatch");
            else getFunctionAt(dispatcher).setName("reference_boot_dispatch", SourceType.USER_DEFINED);
        }
        println("Seeded reference image; physical MCU/PCB compatibility remains unverified.");
    }
}
