// Focused identity, interface and transfer call-site evidence.
// @category AJAZZ
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.util.*;

public class ExportKeyboardUpdaterDetails extends GhidraScript {
    @Override public void run() throws Exception {
        File out=new File(getScriptArgs()[0]);
        Set<Function> selected=new TreeSet<>(Comparator.comparing(f->f.getEntryPoint()));
        for(long va:new long[]{0x407530,0x407ca0,0x423000,0x423f80,0x4252a0,0x426250,
            0x427120,0x428680,0x428810}) {
            Function f=getFunctionAt(toAddr(va));if(f!=null)selected.add(f);
        }
        try(PrintWriter refs=new PrintWriter(new File(out,"interface-references.tsv"),StandardCharsets.UTF_8)) {
            for(long va:new long[]{0x55b1dc,0x55b1e4,0x55b1f4,0x55b1f8,0x5aae50,0x5b6d48})
                for(Reference ref:getReferencesTo(toAddr(va))) {
                    Function f=getFunctionContaining(ref.getFromAddress());
                    refs.println(Long.toHexString(va)+"\t"+ref.getFromAddress()+"\t"+(f==null?"data":f.getEntryPoint()));
                    if(f!=null)selected.add(f);
                }
        }
        for(Function f:new ArrayList<>(selected))for(Function caller:f.getCallingFunctions(monitor))
            if(caller.getEntryPoint().getOffset()<0x42a000)selected.add(caller);
        DecompInterface d=new DecompInterface();d.openProgram(currentProgram);
        try(PrintWriter listing=new PrintWriter(new File(out,"flow-disassembly.txt"),StandardCharsets.UTF_8)) {
            for(Function f:selected) {
                DecompileResults r=d.decompileFunction(f,8,monitor);
                if(r.decompileCompleted()&&r.getDecompiledFunction()!=null)
                    try(PrintWriter file=new PrintWriter(new File(out,f.getEntryPoint()+".c"),StandardCharsets.UTF_8)){
                        file.println(r.getDecompiledFunction().getC());
                    }
                listing.println("\nFUNCTION "+f.getEntryPoint()+" "+f.getName());
                InstructionIterator it=currentProgram.getListing().getInstructions(f.getBody(),true);
                while(it.hasNext()){Instruction i=it.next();listing.println(i.getAddress()+" "+i);}
            }
        } finally {d.dispose();}
        println("Focused evidence functions: "+selected.size());
    }
}
