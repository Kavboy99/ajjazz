"""Observe the running vendor service's device list; no HID commands or changes."""
import http.client
import json
from pathlib import Path

def varint(data, pos):
    value, shift = 0, 0
    while pos < len(data) and shift < 70:
        b=data[pos];pos+=1;value|=(b&127)<<shift
        if not b&128:return value,pos
        shift+=7
    raise ValueError("Invalid protobuf varint")

def fields(data):
    result=[];pos=0
    while pos<len(data):
        key,pos=varint(data,pos);number,wire=key>>3,key&7
        if wire==0:value,pos=varint(data,pos)
        elif wire==2:
            size,pos=varint(data,pos);value=data[pos:pos+size];pos+=size
            if len(value)!=size:raise ValueError("Truncated protobuf")
        else:raise ValueError(f"Unsupported wire type {wire}")
        result.append((number,wire,value))
    return result

def decode(data,depth=0):
    output=[]
    for number,wire,value in fields(data):
        if wire==2:
            try:
                text=value.decode("utf8")
                if all(c.isprintable() for c in text):value=text
                else:raise ValueError()
            except (ValueError,UnicodeDecodeError):
                try:value=decode(value,depth+1) if depth<5 else value.hex()
                except ValueError:value=value.hex()
        output.append({"field":number,"value":value})
    return output

if __name__ == "__main__":
    conn=http.client.HTTPConnection("127.0.0.1",3814,timeout=3)
    try:
        conn.request("POST","/driver.DriverGrpc/watchDevList",body=b"\0\0\0\0\0",headers={"Content-Type":"application/grpc-web+proto","X-Grpc-Web":"1"})
        response=conn.getresponse()
        if response.status!=200:raise RuntimeError(f"HTTP {response.status}")
        header=response.read(5)
        size=int.from_bytes(header[1:],"big")
        if header[0]!=0 or size>1_000_000:raise RuntimeError("Unexpected gRPC frame")
        payload=response.read(size)
        result=decode(payload)
        Path("research/local/driver-inventory.bin").write_bytes(payload)
        Path("research/local/driver-inventory.json").write_text(json.dumps(result,indent=2,ensure_ascii=False),encoding="utf8")
        print(json.dumps(result,indent=2,ensure_ascii=False))
    finally:conn.close()
