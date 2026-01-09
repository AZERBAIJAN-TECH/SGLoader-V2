use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatchClassification {
    pub is_marsey: bool,
    pub is_subverter: bool,
    pub preload: bool,
}

pub fn try_classify_patch(path: &Path) -> Option<PatchClassification> {
    let bytes = std::fs::read(path).ok()?;
    classify_bytes(&bytes).ok().flatten()
}

pub fn try_get_typedef_namespace(path: &Path, type_name: &str) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    typedef_namespace_from_bytes(&bytes, type_name)
        .ok()
        .flatten()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchDisplayInfo {
    pub name: Option<String>,
    pub description: Option<String>,
    pub rdnn: Option<String>,
}

pub fn try_read_patch_display_info(path: &Path) -> Option<PatchDisplayInfo> {
    let bytes = std::fs::read(path).ok()?;
    patch_display_info_from_bytes(&bytes).ok().flatten()
}

fn classify_bytes(bytes: &[u8]) -> Result<Option<PatchClassification>, String> {
    let pe = PeView::parse(bytes)?;
    let Some(cli) = pe.cli_header() else {
        return Ok(None);
    };
    let Some(metadata) = pe.metadata_root(cli.metadata_rva)? else {
        return Ok(None);
    };
    let Some(tables) = metadata.tables_stream()? else {
        return Ok(None);
    };

    let (is_marsey, preload) = tables.has_typedef_with_preload("MarseyPatch")?;
    let (is_subverter, _) = tables.has_typedef_with_preload("SubverterPatch")?;

    if !is_marsey && !is_subverter {
        return Ok(None);
    }

    Ok(Some(PatchClassification {
        is_marsey,
        is_subverter,
        preload,
    }))
}

fn typedef_namespace_from_bytes(bytes: &[u8], type_name: &str) -> Result<Option<String>, String> {
    let pe = PeView::parse(bytes)?;
    let Some(cli) = pe.cli_header() else {
        return Ok(None);
    };
    let Some(metadata) = pe.metadata_root(cli.metadata_rva)? else {
        return Ok(None);
    };
    let Some(tables) = metadata.tables_stream()? else {
        return Ok(None);
    };

    tables.find_typedef_namespace(type_name)
}

fn patch_display_info_from_bytes(bytes: &[u8]) -> Result<Option<PatchDisplayInfo>, String> {
    let pe = PeView::parse(bytes)?;
    let Some(cli) = pe.cli_header() else {
        return Ok(None);
    };
    let Some(metadata) = pe.metadata_root(cli.metadata_rva)? else {
        return Ok(None);
    };
    let Some(tables) = metadata.tables_stream()? else {
        return Ok(None);
    };

    let Some(typedef) = tables
        .find_typedef_ranges("SubverterPatch")?
        .or_else(|| tables.find_typedef_ranges("MarseyPatch").ok().flatten())
    else {
        return Ok(None);
    };

    let Some(cctor) = tables.find_cctor_method(typedef.method_start, typedef.method_end)? else {
        return Ok(Some(PatchDisplayInfo {
            name: None,
            description: None,
            rdnn: None,
        }));
    };

    let Some(method_off) = pe.rva_to_file_offset(cctor.rva) else {
        return Ok(None);
    };

    let Some(code) = read_method_il(bytes, method_off) else {
        return Ok(None);
    };

    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut rdnn: Option<String> = None;

    let mut last_ldstr: Option<String> = None;
    let mut last_newobj_arg: Option<String> = None;

    let mut i = 0usize;
    while i < code.len() {
        let op = code[i];
        i += 1;

        // Two-byte opcodes (0xFE xx) are not needed for our simple scan.
        if op == 0xFE {
            if i < code.len() {
                i += 1;
            }
            continue;
        }

        match op {
            0x72 => {
                // ldstr <token>
                if i + 4 > code.len() {
                    break;
                }
                let token = u32::from_le_bytes([code[i], code[i + 1], code[i + 2], code[i + 3]]);
                i += 4;
                last_ldstr = tables.read_user_string_token(token)?;
            }
            0x73 => {
                // newobj <token>
                if i + 4 > code.len() {
                    break;
                }
                let _token = u32::from_le_bytes([code[i], code[i + 1], code[i + 2], code[i + 3]]);
                i += 4;
                last_newobj_arg = last_ldstr.clone();
            }
            0x80 => {
                // stsfld <field>
                if i + 4 > code.len() {
                    break;
                }
                let token = u32::from_le_bytes([code[i], code[i + 1], code[i + 2], code[i + 3]]);
                i += 4;

                let Some(field_name) = tables.read_field_name_from_token(token)? else {
                    continue;
                };

                if field_name == "Name" {
                    if name.is_none() {
                        name = last_ldstr.clone();
                    }
                } else if field_name == "Description" {
                    if description.is_none() {
                        description = last_ldstr.clone();
                    }
                } else {
                    // Common pattern in patches: Harmony Harm = new("com.example.app");
                    // Capture the string passed to newobj and stored into a field named like "Harm".
                    let lc = field_name.to_ascii_lowercase();
                    if (lc == "harm" || lc.contains("harmony")) && rdnn.is_none() {
                        rdnn = last_newobj_arg.clone();
                    }
                }
            }
            // ret
            0x2A => break,
            _ => {
                // Best-effort: ignore other opcodes.
            }
        }
    }

    Ok(Some(PatchDisplayInfo {
        name,
        description,
        rdnn,
    }))
}

fn read_method_il(bytes: &[u8], method_off: usize) -> Option<&[u8]> {
    if method_off >= bytes.len() {
        return None;
    }
    let b0 = bytes[method_off];
    let kind = b0 & 0x3;

    if kind == 0x2 {
        // Tiny header: 1 byte.
        let code_size = (b0 >> 2) as usize;
        let start = method_off + 1;
        let end = start.saturating_add(code_size);
        if end > bytes.len() {
            return None;
        }
        return Some(&bytes[start..end]);
    }

    if kind != 0x3 {
        return None;
    }

    // Fat header.
    if method_off + 12 > bytes.len() {
        return None;
    }
    let flags_size = u16::from_le_bytes([bytes[method_off], bytes[method_off + 1]]);
    let header_dwords = (flags_size >> 12) as usize;
    if header_dwords == 0 {
        return None;
    }
    let header_size = header_dwords * 4;
    if method_off + header_size > bytes.len() {
        return None;
    }
    let code_size = u32::from_le_bytes([
        bytes[method_off + 4],
        bytes[method_off + 5],
        bytes[method_off + 6],
        bytes[method_off + 7],
    ]) as usize;

    let start = method_off + header_size;
    let end = start.saturating_add(code_size);
    if end > bytes.len() {
        return None;
    }
    Some(&bytes[start..end])
}

#[derive(Debug, Clone, Copy)]
struct CliHeader {
    metadata_rva: u32,
}

#[derive(Debug, Clone, Copy)]
struct Section {
    virtual_address: u32,
    virtual_size: u32,
    raw_ptr: u32,
    raw_size: u32,
}

struct PeView<'a> {
    bytes: &'a [u8],
    sections: Vec<Section>,
    com_descriptor_rva: u32,
}

impl<'a> PeView<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self, String> {
        if bytes.len() < 0x100 {
            return Err("pe too small".to_string());
        }
        if &bytes[0..2] != b"MZ" {
            return Err("missing MZ".to_string());
        }
        let e_lfanew = read_u32(bytes, 0x3c)? as usize;
        if e_lfanew + 4 > bytes.len() {
            return Err("bad e_lfanew".to_string());
        }
        if &bytes[e_lfanew..e_lfanew + 4] != b"PE\0\0" {
            return Err("missing PE signature".to_string());
        }

        let file_header_off = e_lfanew + 4;
        let number_of_sections = read_u16(bytes, file_header_off + 2)? as usize;
        let size_of_optional_header = read_u16(bytes, file_header_off + 16)? as usize;

        let optional_off = file_header_off + 20;
        let magic = read_u16(bytes, optional_off)?;
        let data_dir_start = match magic {
            0x10b => 96usize,  // PE32
            0x20b => 112usize, // PE32+
            _ => return Err("unknown PE optional header magic".to_string()),
        };

        let com_dir_off = optional_off + data_dir_start + 14 * 8;
        if com_dir_off + 8 > bytes.len() {
            return Err("optional header too small (data dir)".to_string());
        }
        let com_rva = read_u32(bytes, com_dir_off)?;
        let _com_size = read_u32(bytes, com_dir_off + 4)?;

        let sections_off = optional_off + size_of_optional_header;
        let mut sections = Vec::with_capacity(number_of_sections);
        for i in 0..number_of_sections {
            let off = sections_off + i * 40;
            if off + 40 > bytes.len() {
                return Err("section table out of range".to_string());
            }
            let virtual_size = read_u32(bytes, off + 8)?;
            let virtual_address = read_u32(bytes, off + 12)?;
            let raw_size = read_u32(bytes, off + 16)?;
            let raw_ptr = read_u32(bytes, off + 20)?;
            sections.push(Section {
                virtual_address,
                virtual_size,
                raw_ptr,
                raw_size,
            });
        }

        Ok(Self {
            bytes,
            sections,
            com_descriptor_rva: com_rva,
        })
    }

    fn rva_to_file_offset(&self, rva: u32) -> Option<usize> {
        for s in &self.sections {
            let size = s.virtual_size.max(s.raw_size);
            if size == 0 {
                continue;
            }
            if rva >= s.virtual_address && rva < s.virtual_address.saturating_add(size) {
                let delta = rva - s.virtual_address;
                return Some((s.raw_ptr + delta) as usize);
            }
        }
        None
    }

    fn cli_header(&self) -> Option<CliHeader> {
        if self.com_descriptor_rva == 0 {
            return None;
        }
        let cli_off = self.rva_to_file_offset(self.com_descriptor_rva)?;
        if cli_off + 16 > self.bytes.len() {
            return None;
        }
        // IMAGE_COR20_HEADER.MetaData is at +8 (RVA,Size)
        let metadata_rva = read_u32(self.bytes, cli_off + 8).ok()?;
        Some(CliHeader { metadata_rva })
    }

    fn metadata_root(&self, metadata_rva: u32) -> Result<Option<MetadataRoot<'a>>, String> {
        let Some(meta_off) = self.rva_to_file_offset(metadata_rva) else {
            return Ok(None);
        };
        if meta_off + 16 > self.bytes.len() {
            return Ok(None);
        }
        let b = self.bytes;
        if &b[meta_off..meta_off + 4] != b"BSJB" {
            return Ok(None);
        }

        let ver_len = read_u32(b, meta_off + 12)? as usize;
        let mut cursor = meta_off + 16;
        if cursor + ver_len > b.len() {
            return Ok(None);
        }
        cursor += ver_len;
        cursor = (cursor + 3) & !3;

        if cursor + 4 > b.len() {
            return Ok(None);
        }
        let _flags = read_u16(b, cursor)?;
        let streams = read_u16(b, cursor + 2)? as usize;
        cursor += 4;

        let mut strings = None;
        let mut blob = None;
        let mut us = None;
        let mut tables = None;

        for _ in 0..streams {
            if cursor + 8 > b.len() {
                return Ok(None);
            }
            let off = read_u32(b, cursor)? as usize;
            let size = read_u32(b, cursor + 4)? as usize;
            cursor += 8;

            let name_start = cursor;
            while cursor < b.len() && b[cursor] != 0 {
                cursor += 1;
            }
            if cursor >= b.len() {
                return Ok(None);
            }
            let name = std::str::from_utf8(&b[name_start..cursor])
                .map_err(|_| "bad stream name utf8".to_string())?
                .to_string();
            cursor += 1;
            cursor = (cursor + 3) & !3;

            let abs_off = meta_off + off;
            if abs_off > b.len() || abs_off + size > b.len() {
                continue;
            }

            match name.as_str() {
                "#Strings" => strings = Some((abs_off, size)),
                "#Blob" => blob = Some((abs_off, size)),
                "#US" => us = Some((abs_off, size)),
                "#~" | "#-" => tables = Some((abs_off, size)),
                _ => {}
            }
        }

        Ok(Some(MetadataRoot {
            bytes: self.bytes,
            strings,
            blob,
            us,
            tables,
        }))
    }
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct CctorMethod {
    rva: u32,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct TypedefRanges {
    method_start: u32,
    method_end: u32,
}

struct MetadataRoot<'a> {
    bytes: &'a [u8],
    strings: Option<(usize, usize)>,
    blob: Option<(usize, usize)>,
    us: Option<(usize, usize)>,
    tables: Option<(usize, usize)>,
}

impl<'a> MetadataRoot<'a> {
    fn tables_stream(&self) -> Result<Option<TablesStream<'a>>, String> {
        let Some((tables_off, tables_size)) = self.tables else {
            return Ok(None);
        };
        let Some((strings_off, strings_size)) = self.strings else {
            return Ok(None);
        };
        let Some((blob_off, blob_size)) = self.blob else {
            return Ok(None);
        };
        let (us_off, us_size) = self.us.unwrap_or((0, 0));
        Ok(Some(TablesStream::parse(
            self.bytes,
            tables_off,
            tables_size,
            strings_off,
            strings_size,
            blob_off,
            blob_size,
            us_off,
            us_size,
        )?))
    }
}

struct TablesStream<'a> {
    bytes: &'a [u8],
    strings_off: usize,
    strings_size: usize,
    blob_off: usize,
    blob_size: usize,
    us_off: usize,
    us_size: usize,
    tables_data_off: usize,
    heap_sizes: u8,
    rows: [u32; 64],
}

impl<'a> TablesStream<'a> {
    fn parse(
        bytes: &'a [u8],
        tables_off: usize,
        tables_size: usize,
        strings_off: usize,
        strings_size: usize,
        blob_off: usize,
        blob_size: usize,
        us_off: usize,
        us_size: usize,
    ) -> Result<Self, String> {
        if tables_off + 24 > bytes.len() {
            return Err("tables stream too small".to_string());
        }
        let heap_sizes = bytes[tables_off + 6];
        let valid = read_u64(bytes, tables_off + 8)?;
        let mut cursor = tables_off + 24;

        let mut rows = [0u32; 64];
        for (tid, row) in rows.iter_mut().enumerate() {
            if (valid >> tid) & 1 == 1 {
                *row = read_u32(bytes, cursor)?;
                cursor += 4;
            }
        }

        let end = tables_off + tables_size;
        if cursor > end {
            return Err("tables row counts exceed stream".to_string());
        }

        Ok(Self {
            bytes,
            strings_off,
            strings_size,
            blob_off,
            blob_size,
            us_off,
            us_size,
            tables_data_off: cursor,
            heap_sizes,
            rows,
        })
    }

    fn read_user_string_token(&self, token: u32) -> Result<Option<String>, String> {
        // User string token: 0x70xxxxxx (offset into #US).
        if (token >> 24) != 0x70 {
            return Ok(None);
        }
        if self.us_off == 0 || self.us_size == 0 {
            return Ok(None);
        }
        let idx = (token & 0x00FF_FFFF) as usize;
        let start = self.us_off + idx;
        if start >= self.bytes.len() || start < self.us_off || start >= self.us_off + self.us_size {
            return Ok(None);
        }
        let (len, hdr) = read_compressed_u32(self.bytes, start)?;
        let data_start = start + hdr;
        let data_end = data_start.saturating_add(len as usize);
        if data_end > self.bytes.len() {
            return Ok(None);
        }
        let blob = &self.bytes[data_start..data_end];
        if blob.is_empty() {
            return Ok(Some(String::new()));
        }

        // Per ECMA-335, user string ends with a single byte (string kind/flags).
        let utf16_bytes = if !blob.is_empty() {
            &blob[..blob.len() - 1]
        } else {
            blob
        };
        let mut units: Vec<u16> = Vec::with_capacity(utf16_bytes.len() / 2);
        let mut p = 0usize;
        while p + 1 < utf16_bytes.len() {
            units.push(u16::from_le_bytes([utf16_bytes[p], utf16_bytes[p + 1]]));
            p += 2;
        }
        let mut s = String::from_utf16_lossy(&units);
        while s.ends_with('\0') {
            s.pop();
        }
        Ok(Some(s))
    }

    fn read_field_name_from_token(&self, token: u32) -> Result<Option<String>, String> {
        if (token >> 24) != 0x04 {
            return Ok(None);
        }
        let idx = token & 0x00FF_FFFF;
        if idx == 0 {
            return Ok(None);
        }
        self.read_field_name(idx)
    }

    fn read_field_name(&self, field_row: u32) -> Result<Option<String>, String> {
        if field_row == 0 || field_row > self.rows[4] {
            return Ok(None);
        }

        let string_index_size = if (self.heap_sizes & 0x01) != 0 { 4 } else { 2 };
        let blob_index_size = if (self.heap_sizes & 0x04) != 0 { 4 } else { 2 };
        let field_row_size = 2 + string_index_size + blob_index_size;

        let field_index_size = table_index_size(self.rows[4]);
        let _method_index_size = table_index_size(self.rows[6]);
        let typedef_or_ref_size = coded_index_size(2, &[2, 1, 27], &self.rows);
        let resolution_scope_size = coded_index_size(2, &[0, 26, 35, 1], &self.rows);
        let guid_index_size = if (self.heap_sizes & 0x02) != 0 { 4 } else { 2 };

        let module_row_size = 2 + string_index_size + guid_index_size * 3;
        let typeref_row_size = resolution_scope_size + string_index_size + string_index_size;
        let typedef_row_size = 4
            + string_index_size
            + string_index_size
            + typedef_or_ref_size
            + field_index_size
            + table_index_size(self.rows[6]);
        let fieldptr_row_size = field_index_size;

        // Up to Field.
        let mut cur = 0usize;
        cur += (self.rows[0] as usize) * module_row_size;
        cur += (self.rows[1] as usize) * typeref_row_size;
        cur += (self.rows[2] as usize) * typedef_row_size;
        cur += (self.rows[3] as usize) * fieldptr_row_size;
        let field_start = cur;

        let foff = self.tables_data_off + field_start + ((field_row - 1) as usize) * field_row_size;
        if foff + field_row_size > self.bytes.len() {
            return Ok(None);
        }
        let p = foff + 2;
        let fname_idx = read_index(self.bytes, p, string_index_size)?;
        let fname = self.read_string(fname_idx)?;
        if fname.is_empty() {
            return Ok(None);
        }
        Ok(Some(fname))
    }

    fn read_method_name_and_rva(&self, method_row: u32) -> Result<Option<(String, u32)>, String> {
        if method_row == 0 || method_row > self.rows[6] {
            return Ok(None);
        }

        let string_index_size = if (self.heap_sizes & 0x01) != 0 { 4 } else { 2 };
        let blob_index_size = if (self.heap_sizes & 0x04) != 0 { 4 } else { 2 };
        let guid_index_size = if (self.heap_sizes & 0x02) != 0 { 4 } else { 2 };

        let field_index_size = table_index_size(self.rows[4]);
        let method_index_size = table_index_size(self.rows[6]);
        let param_index_size = table_index_size(self.rows[8]);

        let typedef_or_ref_size = coded_index_size(2, &[2, 1, 27], &self.rows);
        let resolution_scope_size = coded_index_size(2, &[0, 26, 35, 1], &self.rows);

        let module_row_size = 2 + string_index_size + guid_index_size * 3;
        let typeref_row_size = resolution_scope_size + string_index_size + string_index_size;
        let typedef_row_size = 4
            + string_index_size
            + string_index_size
            + typedef_or_ref_size
            + field_index_size
            + method_index_size;
        let fieldptr_row_size = field_index_size;
        let field_row_size = 2 + string_index_size + blob_index_size;
        let methodptr_row_size = method_index_size;
        let methoddef_row_size = 4 + 2 + 2 + string_index_size + blob_index_size + param_index_size;

        // Up to MethodDef.
        let mut cur = 0usize;
        cur += (self.rows[0] as usize) * module_row_size;
        cur += (self.rows[1] as usize) * typeref_row_size;
        cur += (self.rows[2] as usize) * typedef_row_size;
        cur += (self.rows[3] as usize) * fieldptr_row_size;
        cur += (self.rows[4] as usize) * field_row_size;
        cur += (self.rows[5] as usize) * methodptr_row_size;
        let methoddef_start = cur;

        let off = self.tables_data_off
            + methoddef_start
            + ((method_row - 1) as usize) * methoddef_row_size;
        if off + methoddef_row_size > self.bytes.len() {
            return Ok(None);
        }
        let rva = read_u32(self.bytes, off)?;
        let p = off + 4 + 2 + 2;
        let name_idx = read_index(self.bytes, p, string_index_size)?;
        let name = self.read_string(name_idx)?;
        if name.is_empty() {
            return Ok(None);
        }
        Ok(Some((name, rva)))
    }

    fn find_cctor_method(&self, start: u32, end: u32) -> Result<Option<CctorMethod>, String> {
        if start == 0 || start >= end {
            return Ok(None);
        }

        let method_index_size = table_index_size(self.rows[6]);
        let methodptr_present = self.rows[5] > 0;
        let methodptr_start = self.methodptr_table_start()?;

        for logical_idx in start..end {
            let method_row = if methodptr_present {
                let ptr_off = self.tables_data_off
                    + methodptr_start
                    + ((logical_idx - 1) as usize) * method_index_size;
                if ptr_off + method_index_size > self.bytes.len() {
                    continue;
                }
                read_index(self.bytes, ptr_off, method_index_size)?
            } else {
                logical_idx
            };

            let Some((name, rva)) = self.read_method_name_and_rva(method_row)? else {
                continue;
            };
            if name == ".cctor" {
                return Ok(Some(CctorMethod { rva }));
            }
        }

        Ok(None)
    }

    fn methodptr_table_start(&self) -> Result<usize, String> {
        let string_index_size = if (self.heap_sizes & 0x01) != 0 { 4 } else { 2 };
        let blob_index_size = if (self.heap_sizes & 0x04) != 0 { 4 } else { 2 };
        let guid_index_size = if (self.heap_sizes & 0x02) != 0 { 4 } else { 2 };

        let field_index_size = table_index_size(self.rows[4]);
        let method_index_size = table_index_size(self.rows[6]);

        let typedef_or_ref_size = coded_index_size(2, &[2, 1, 27], &self.rows);
        let resolution_scope_size = coded_index_size(2, &[0, 26, 35, 1], &self.rows);

        let module_row_size = 2 + string_index_size + guid_index_size * 3;
        let typeref_row_size = resolution_scope_size + string_index_size + string_index_size;
        let typedef_row_size = 4
            + string_index_size
            + string_index_size
            + typedef_or_ref_size
            + field_index_size
            + method_index_size;
        let fieldptr_row_size = field_index_size;
        let field_row_size = 2 + string_index_size + blob_index_size;

        let mut cur = 0usize;
        cur += (self.rows[0] as usize) * module_row_size;
        cur += (self.rows[1] as usize) * typeref_row_size;
        cur += (self.rows[2] as usize) * typedef_row_size;
        cur += (self.rows[3] as usize) * fieldptr_row_size;
        cur += (self.rows[4] as usize) * field_row_size;
        Ok(cur)
    }

    fn find_typedef_ranges(&self, type_name: &str) -> Result<Option<TypedefRanges>, String> {
        let string_index_size = if (self.heap_sizes & 0x01) != 0 { 4 } else { 2 };
        let guid_index_size = if (self.heap_sizes & 0x02) != 0 { 4 } else { 2 };

        let field_index_size = table_index_size(self.rows[4]);
        let method_index_size = table_index_size(self.rows[6]);

        let typedef_or_ref_size = coded_index_size(2, &[2, 1, 27], &self.rows);
        let resolution_scope_size = coded_index_size(2, &[0, 26, 35, 1], &self.rows);

        let module_row_size = 2 + string_index_size + guid_index_size * 3;
        let typeref_row_size = resolution_scope_size + string_index_size + string_index_size;
        let typedef_row_size = 4
            + string_index_size
            + string_index_size
            + typedef_or_ref_size
            + field_index_size
            + method_index_size;

        let mut cur = 0usize;
        cur += (self.rows[0] as usize) * module_row_size;
        cur += (self.rows[1] as usize) * typeref_row_size;
        let typedef_start = cur;

        let typedef_count = self.rows[2] as usize;
        if typedef_count == 0 {
            return Ok(None);
        }

        let mut wanted_pos: Option<usize> = None;
        let mut fieldlists: Vec<u32> = Vec::with_capacity(typedef_count);
        let mut methodlists: Vec<u32> = Vec::with_capacity(typedef_count);

        for i in 0..typedef_count {
            let off = self.tables_data_off + typedef_start + i * typedef_row_size;
            if off + typedef_row_size > self.bytes.len() {
                break;
            }

            let mut p = off + 4;
            let name_idx = read_index(self.bytes, p, string_index_size)?;
            p += string_index_size;
            p += string_index_size; // ns
            p += typedef_or_ref_size;
            let fieldlist = read_index(self.bytes, p, field_index_size)?;
            p += field_index_size;
            let methodlist = read_index(self.bytes, p, method_index_size)?;

            fieldlists.push(fieldlist);
            methodlists.push(methodlist);

            let name = self.read_string(name_idx)?;
            if name == type_name {
                wanted_pos = Some(i);
            }
        }

        let Some(pos) = wanted_pos else {
            return Ok(None);
        };

        let method_start = methodlists[pos];
        let method_end = if pos + 1 < methodlists.len() {
            methodlists[pos + 1]
        } else if self.rows[5] > 0 {
            self.rows[5].saturating_add(1)
        } else {
            self.rows[6].saturating_add(1)
        };

        Ok(Some(TypedefRanges {
            method_start,
            method_end,
        }))
    }

    fn has_typedef_with_preload(&self, type_name: &str) -> Result<(bool, bool), String> {
        let string_index_size = if (self.heap_sizes & 0x01) != 0 { 4 } else { 2 };
        let guid_index_size = if (self.heap_sizes & 0x02) != 0 { 4 } else { 2 };
        let blob_index_size = if (self.heap_sizes & 0x04) != 0 { 4 } else { 2 };

        let field_index_size = table_index_size(self.rows[4]);
        let method_index_size = table_index_size(self.rows[6]);

        let typedef_or_ref_size = coded_index_size(2, &[2, 1, 27], &self.rows);
        let resolution_scope_size = coded_index_size(2, &[0, 26, 35, 1], &self.rows);

        let module_row_size = 2 + string_index_size + guid_index_size * 3;
        let typeref_row_size = resolution_scope_size + string_index_size + string_index_size;
        let typedef_row_size = 4
            + string_index_size
            + string_index_size
            + typedef_or_ref_size
            + field_index_size
            + method_index_size;
        let fieldptr_row_size = field_index_size;
        let field_row_size = 2 + string_index_size + blob_index_size;

        // table order: Module(0), TypeRef(1), TypeDef(2), FieldPtr(3), Field(4)
        let mut cur = 0usize;

        let _module_start = cur;
        cur += (self.rows[0] as usize) * module_row_size;

        let _typeref_start = cur;
        cur += (self.rows[1] as usize) * typeref_row_size;

        let typedef_start = cur;
        cur += (self.rows[2] as usize) * typedef_row_size;

        let fieldptr_present = self.rows[3] > 0;
        let fieldptr_start = cur;
        cur += (self.rows[3] as usize) * fieldptr_row_size;

        let field_start = cur;

        let typedef_count = self.rows[2] as usize;
        if typedef_count == 0 {
            return Ok((false, false));
        }

        let mut wanted_pos: Option<usize> = None;
        let mut fieldlists: Vec<u32> = Vec::with_capacity(typedef_count);

        for i in 0..typedef_count {
            let off = self.tables_data_off + typedef_start + i * typedef_row_size;
            if off + typedef_row_size > self.bytes.len() {
                break;
            }

            let mut p = off + 4; // skip flags
            let name_idx = read_index(self.bytes, p, string_index_size)?;
            p += string_index_size;
            let _ns_idx = read_index(self.bytes, p, string_index_size)?;
            p += string_index_size;
            p += typedef_or_ref_size;
            let fieldlist = read_index(self.bytes, p, field_index_size)?;
            p += field_index_size;
            let _methodlist = read_index(self.bytes, p, method_index_size)?;

            fieldlists.push(fieldlist);

            let name = self.read_string(name_idx)?;
            if name == type_name {
                wanted_pos = Some(i);
            }
        }

        let Some(pos) = wanted_pos else {
            return Ok((false, false));
        };

        let start = fieldlists[pos];
        if start == 0 {
            return Ok((true, false));
        }

        let end = if pos + 1 < fieldlists.len() {
            fieldlists[pos + 1]
        } else if fieldptr_present {
            self.rows[3].saturating_add(1)
        } else {
            self.rows[4].saturating_add(1)
        };

        if start >= end {
            return Ok((true, false));
        }

        let mut preload = false;
        for logical_idx in start..end {
            let field_row = if fieldptr_present {
                let ptr_off = self.tables_data_off
                    + fieldptr_start
                    + ((logical_idx - 1) as usize) * fieldptr_row_size;
                if ptr_off + fieldptr_row_size > self.bytes.len() {
                    continue;
                }
                read_index(self.bytes, ptr_off, field_index_size)?
            } else {
                logical_idx
            };

            if field_row == 0 || field_row > self.rows[4] {
                continue;
            }

            let foff =
                self.tables_data_off + field_start + ((field_row - 1) as usize) * field_row_size;
            if foff + field_row_size > self.bytes.len() {
                continue;
            }

            let mut p = foff + 2; // flags u16
            let fname_idx = read_index(self.bytes, p, string_index_size)?;
            p += string_index_size;
            let fsig_idx = read_index(self.bytes, p, blob_index_size)?;

            let fname = self.read_string(fname_idx)?;
            if fname != "preload" {
                continue;
            }

            if let Some(sig) = self.read_blob(fsig_idx)? {
                // FieldSig ::= 0x06 <type>
                // bool element type is 0x02
                if sig.len() >= 2 && sig[0] == 0x06 && sig[1] == 0x02 {
                    preload = true;
                }
            }
        }

        Ok((true, preload))
    }

    fn find_typedef_namespace(&self, type_name: &str) -> Result<Option<String>, String> {
        let string_index_size = if (self.heap_sizes & 0x01) != 0 { 4 } else { 2 };
        let guid_index_size = if (self.heap_sizes & 0x02) != 0 { 4 } else { 2 };

        let field_index_size = table_index_size(self.rows[4]);
        let method_index_size = table_index_size(self.rows[6]);

        let typedef_or_ref_size = coded_index_size(2, &[2, 1, 27], &self.rows);
        let resolution_scope_size = coded_index_size(2, &[0, 26, 35, 1], &self.rows);

        let module_row_size = 2 + string_index_size + guid_index_size * 3;
        let typeref_row_size = resolution_scope_size + string_index_size + string_index_size;
        let typedef_row_size = 4
            + string_index_size
            + string_index_size
            + typedef_or_ref_size
            + field_index_size
            + method_index_size;

        // table order: Module(0), TypeRef(1), TypeDef(2)
        let mut cur = 0usize;
        cur += (self.rows[0] as usize) * module_row_size;
        cur += (self.rows[1] as usize) * typeref_row_size;
        let typedef_start = cur;

        let typedef_count = self.rows[2] as usize;
        if typedef_count == 0 {
            return Ok(None);
        }

        for i in 0..typedef_count {
            let off = self.tables_data_off + typedef_start + i * typedef_row_size;
            if off + typedef_row_size > self.bytes.len() {
                break;
            }

            let mut p = off + 4; // skip flags
            let name_idx = read_index(self.bytes, p, string_index_size)?;
            p += string_index_size;
            let ns_idx = read_index(self.bytes, p, string_index_size)?;

            let name = self.read_string(name_idx)?;
            if name != type_name {
                continue;
            }

            let ns = self.read_string(ns_idx)?;
            if ns.is_empty() {
                return Ok(None);
            }

            return Ok(Some(ns));
        }

        Ok(None)
    }

    fn read_string(&self, idx: u32) -> Result<String, String> {
        if idx == 0 {
            return Ok(String::new());
        }
        let off = self.strings_off + idx as usize;
        if off >= self.bytes.len()
            || off < self.strings_off
            || off >= self.strings_off + self.strings_size
        {
            return Ok(String::new());
        }
        let mut end = off;
        while end < self.bytes.len()
            && end < self.strings_off + self.strings_size
            && self.bytes[end] != 0
        {
            end += 1;
        }
        std::str::from_utf8(&self.bytes[off..end])
            .map(|s| s.to_string())
            .map_err(|_| "bad string heap utf8".to_string())
    }

    fn read_blob(&self, idx: u32) -> Result<Option<&'a [u8]>, String> {
        if idx == 0 {
            return Ok(None);
        }
        let start = self.blob_off + idx as usize;
        if start >= self.bytes.len()
            || start < self.blob_off
            || start >= self.blob_off + self.blob_size
        {
            return Ok(None);
        }
        let (len, hdr) = read_compressed_u32(self.bytes, start)?;
        let data_start = start + hdr;
        let data_end = data_start.saturating_add(len as usize);
        if data_start > self.bytes.len() || data_end > self.bytes.len() {
            return Ok(None);
        }
        Ok(Some(&self.bytes[data_start..data_end]))
    }
}

fn table_index_size(rows: u32) -> usize {
    if rows > 0xFFFF { 4 } else { 2 }
}

fn coded_index_size(tag_bits: u32, tables: &[usize], rows: &[u32; 64]) -> usize {
    let max_rows = tables.iter().map(|&t| rows[t]).max().unwrap_or(0);
    let limit = 1u32 << (16 - tag_bits);
    if max_rows < limit { 2 } else { 4 }
}

fn read_index(bytes: &[u8], off: usize, size: usize) -> Result<u32, String> {
    match size {
        2 => Ok(read_u16(bytes, off)? as u32),
        4 => read_u32(bytes, off),
        _ => Err("bad index size".to_string()),
    }
}

fn read_u16(bytes: &[u8], off: usize) -> Result<u16, String> {
    if off + 2 > bytes.len() {
        return Err("oob u16".to_string());
    }
    Ok(u16::from_le_bytes([bytes[off], bytes[off + 1]]))
}

fn read_u32(bytes: &[u8], off: usize) -> Result<u32, String> {
    if off + 4 > bytes.len() {
        return Err("oob u32".to_string());
    }
    Ok(u32::from_le_bytes([
        bytes[off],
        bytes[off + 1],
        bytes[off + 2],
        bytes[off + 3],
    ]))
}

fn read_u64(bytes: &[u8], off: usize) -> Result<u64, String> {
    if off + 8 > bytes.len() {
        return Err("oob u64".to_string());
    }
    Ok(u64::from_le_bytes([
        bytes[off],
        bytes[off + 1],
        bytes[off + 2],
        bytes[off + 3],
        bytes[off + 4],
        bytes[off + 5],
        bytes[off + 6],
        bytes[off + 7],
    ]))
}

// ECMA-335 II.23.2: compressed unsigned integer
fn read_compressed_u32(bytes: &[u8], off: usize) -> Result<(u32, usize), String> {
    if off >= bytes.len() {
        return Err("oob compressed int".to_string());
    }
    let b0 = bytes[off];
    if (b0 & 0x80) == 0 {
        return Ok((b0 as u32, 1));
    }
    if (b0 & 0xC0) == 0x80 {
        if off + 2 > bytes.len() {
            return Err("oob compressed int (2)".to_string());
        }
        let b1 = bytes[off + 1];
        let v = (((b0 & 0x3F) as u32) << 8) | (b1 as u32);
        return Ok((v, 2));
    }
    if (b0 & 0xE0) == 0xC0 {
        if off + 4 > bytes.len() {
            return Err("oob compressed int (4)".to_string());
        }
        let b1 = bytes[off + 1];
        let b2 = bytes[off + 2];
        let b3 = bytes[off + 3];
        let v =
            (((b0 & 0x1F) as u32) << 24) | ((b1 as u32) << 16) | ((b2 as u32) << 8) | (b3 as u32);
        return Ok((v, 4));
    }
    Err("invalid compressed int".to_string())
}
