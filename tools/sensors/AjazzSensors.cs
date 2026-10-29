// Read-only temperature bridge. No control interfaces, fan curves or settings.
using System;
using System.Collections.Generic;
using System.Web.Script.Serialization;
using LibreHardwareMonitor.Hardware;

public static class AjazzSensors {
    static int? Temperature(float? v) {
        if(!v.HasValue || float.IsNaN(v.Value) || float.IsInfinity(v.Value) || v.Value < -10 || v.Value > 150)return null;
        return (int)Math.Round(v.Value*10);
    }
    static void Read(IHardware h,Dictionary<string,object> values) {
        h.Update();
        foreach(var s in h.Sensors) {
            if(s.SensorType!=SensorType.Temperature)continue;
            var v=Temperature(s.Value);if(!v.HasValue)continue;
            string key=null;
            if(h.HardwareType==HardwareType.Cpu) {
                if(v.Value<=0)continue;
                if(s.Name.IndexOf("Tctl",StringComparison.OrdinalIgnoreCase)>=0 || s.Name.IndexOf("Package",StringComparison.OrdinalIgnoreCase)>=0)key="cpu";
            } else if(h.HardwareType==HardwareType.Storage)key="storage";
            else if(h.HardwareType==HardwareType.SuperIO || h.HardwareType==HardwareType.Motherboard) {
                if(s.Name.IndexOf("System",StringComparison.OrdinalIgnoreCase)>=0 || s.Name.IndexOf("Motherboard",StringComparison.OrdinalIgnoreCase)>=0)key="board";
            }
            if(key!=null && (values[key]==null || (int)values[key]<v.Value))values[key]=v.Value;
        }
        foreach(var sub in h.SubHardware)Read(sub,values);
    }
    public static int Main() {
        var json=new JavaScriptSerializer();
        var computer=new Computer {IsCpuEnabled=true,IsMotherboardEnabled=true,IsGpuEnabled=false,IsStorageEnabled=true,IsMemoryEnabled=false,IsControllerEnabled=false,IsNetworkEnabled=false};
        try {
            computer.Open();
            for(int i=0;i<12 && Console.ReadLine()!=null;i++) {
                var values=new Dictionary<string,object>{{"cpu",null},{"gpu",null},{"storage",null},{"board",null}};
                foreach(var h in computer.Hardware) {try {Read(h,values);}catch{}}
                Console.WriteLine(json.Serialize(values));Console.Out.Flush();
            }
        } catch {return 1;} finally {try {computer.Close();}catch{}}
        return 0;
    }
}
