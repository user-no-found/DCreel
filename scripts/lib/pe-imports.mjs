export function peImports(bytes) {
  function requireRange(offset, size) {
    if (!Number.isSafeInteger(offset) || offset < 0 || offset + size > bytes.length) {
      throw new Error("PE 数据越界或已截断");
    }
  }
  function u16(offset) {
    requireRange(offset, 2);
    return bytes.readUInt16LE(offset);
  }
  function u32(offset) {
    requireRange(offset, 4);
    return bytes.readUInt32LE(offset);
  }
  if (u16(0) !== 0x5a4d) throw new Error("不是 Windows PE 文件");
  const pe = u32(0x3c);
  if (u32(pe) !== 0x4550) throw new Error("PE 签名无效");
  const sectionCount = u16(pe + 6);
  const optionalSize = u16(pe + 20);
  const optional = pe + 24;
  requireRange(optional, optionalSize);
  const magic = u16(optional);
  if (magic !== 0x10b && magic !== 0x20b) throw new Error("未知 PE 可选头格式");
  const directoryOffset = magic === 0x20b ? 112 : 96;
  if (optionalSize < directoryOffset) throw new Error("PE 可选头不完整");
  const directoryCount = u32(optional + directoryOffset - 4);
  if (directoryCount > 16 || directoryOffset + directoryCount * 8 > optionalSize) {
    throw new Error("PE 数据目录无效");
  }
  const sections = [];
  for (let index = 0; index < sectionCount; index++) {
    const section = optional + optionalSize + index * 40;
    requireRange(section, 40);
    sections.push({ rva: u32(section + 12), size: u32(section + 16), raw: u32(section + 20) });
  }
  function fileOffset(rva, size) {
    const section = sections.find((entry) => rva >= entry.rva && rva - entry.rva + size <= entry.size);
    if (!section) throw new Error("PE 导入数据未映射到文件");
    const offset = section.raw + rva - section.rva;
    requireRange(offset, size);
    return offset;
  }
  function dllName(rva) {
    const characters = [];
    for (let index = 0; index < 260; index++) {
      const value = bytes[fileOffset(rva + index, 1)];
      if (value === 0) {
        if (!characters.length) throw new Error("PE 导入 DLL 名称为空");
        return String.fromCharCode(...characters);
      }
      if (value < 32 || value > 126) throw new Error("PE 导入 DLL 名称无效");
      characters.push(value);
    }
    throw new Error("PE 导入 DLL 名称未终止");
  }
  const imports = new Set();
  for (const [directoryIndex, descriptorSize, nameOffset] of [[1, 20, 12], [13, 32, 4]]) {
    if (directoryIndex >= directoryCount) continue;
    const directory = optional + directoryOffset + directoryIndex * 8;
    const rva = u32(directory);
    const size = u32(directory + 4);
    if (!rva && !size) continue;
    if (!rva || size < descriptorSize) throw new Error("PE 导入目录无效");
    let terminated = false;
    for (let index = 0; index + descriptorSize <= size; index += descriptorSize) {
      const descriptor = fileOffset(rva + index, descriptorSize);
      if (bytes.subarray(descriptor, descriptor + descriptorSize).every((value) => value === 0)) {
        terminated = true;
        break;
      }
      if (directoryIndex === 13 && (u32(descriptor) & 1) === 0) {
        throw new Error("PE 延迟导入使用了不支持的绝对地址格式");
      }
      imports.add(dllName(u32(descriptor + nameOffset)));
    }
    if (!terminated) throw new Error("PE 导入目录未终止");
  }
  return [...imports].sort();
}

export function externalVcRuntimes(imports) {
  return imports.filter((name) => /^(?:vcruntime\d+(?:_\d+)*d?|msvcp\d+(?:_\w+)*d?|concrt\d+d?|ucrtbased)\.dll$/i.test(name));
}
