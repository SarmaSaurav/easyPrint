use anyhow::Result;
use bytes::{BufMut, Bytes, BytesMut};

// IPP Delimiter Tags
pub const TAG_OPERATION_ATTRIBUTES: u8 = 0x01;
pub const TAG_JOB_ATTRIBUTES: u8 = 0x02;
pub const TAG_END_OF_ATTRIBUTES: u8 = 0x03;
pub const TAG_PRINTER_ATTRIBUTES: u8 = 0x04;

// IPP Value Tags
pub const TAG_INTEGER: u8 = 0x21;
pub const TAG_BOOLEAN: u8 = 0x22;
pub const TAG_ENUM: u8 = 0x23;
pub const TAG_OCTET_STRING: u8 = 0x30;
pub const TAG_TEXT_WITHOUT_LANG: u8 = 0x41;
pub const TAG_NAME_WITHOUT_LANG: u8 = 0x42;
pub const TAG_KEYWORD: u8 = 0x44;
pub const TAG_URI: u8 = 0x45;
pub const TAG_CHARSET: u8 = 0x47;
pub const TAG_NATURAL_LANG: u8 = 0x48;
pub const TAG_MIME_TYPE: u8 = 0x49;

// IPP Operations
pub const OP_PRINT_JOB: u16 = 0x0002;
pub const OP_VALIDATE_JOB: u16 = 0x0004;
pub const OP_GET_JOB_ATTRIBUTES: u16 = 0x0009;
pub const OP_GET_JOBS: u16 = 0x000a;
pub const OP_GET_PRINTER_ATTRIBUTES: u16 = 0x000b;

// IPP Status Codes
pub const STATUS_OK: u16 = 0x0000;
pub const STATUS_ERROR_NOT_FOUND: u16 = 0x0406;

#[derive(Debug)]
pub struct IppRequest {
    pub version_major: u8,
    pub version_minor: u8,
    pub operation_id: u16,
    pub request_id: u32,
    pub document_data: Vec<u8>,
}

impl IppRequest {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 8 {
            return Err(anyhow::anyhow!("IPP payload too short"));
        }

        let version_major = data[0];
        let version_minor = data[1];
        let operation_id = u16::from_be_bytes([data[2], data[3]]);
        let request_id = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);

        // Find end of attributes tag (0x03)
        let mut doc_start = data.len();
        for i in 8..data.len() {
            if data[i] == TAG_END_OF_ATTRIBUTES {
                doc_start = i + 1;
                break;
            }
        }

        let document_data = if doc_start < data.len() {
            data[doc_start..].to_vec()
        } else {
            Vec::new()
        };

        Ok(Self {
            version_major,
            version_minor,
            operation_id,
            request_id,
            document_data,
        })
    }
}

pub struct IppResponseBuilder {
    buf: BytesMut,
}

impl IppResponseBuilder {
    pub fn new(major: u8, minor: u8, status_code: u16, request_id: u32) -> Self {
        let mut buf = BytesMut::with_capacity(1024);
        buf.put_u8(major);
        buf.put_u8(minor);
        buf.put_u16(status_code);
        buf.put_u32(request_id);

        let mut builder = Self { buf };
        // Every IPP response begins with operation-attributes-tag
        builder.buf.put_u8(TAG_OPERATION_ATTRIBUTES);
        builder.add_attribute(TAG_CHARSET, "attributes-charset", "utf-8".as_bytes());
        builder.add_attribute(
            TAG_NATURAL_LANG,
            "attributes-natural-language",
            "en".as_bytes(),
        );

        builder
    }

    pub fn start_group(&mut self, group_tag: u8) {
        self.buf.put_u8(group_tag);
    }

    pub fn add_attribute(&mut self, value_tag: u8, name: &str, value: &[u8]) {
        self.buf.put_u8(value_tag);
        self.buf.put_u16(name.len() as u16);
        self.buf.put_slice(name.as_bytes());
        self.buf.put_u16(value.len() as u16);
        self.buf.put_slice(value);
    }

    pub fn add_sub_attribute(&mut self, value_tag: u8, value: &[u8]) {
        self.buf.put_u8(value_tag);
        self.buf.put_u16(0); // 0 length name indicates multi-value
        self.buf.put_u16(value.len() as u16);
        self.buf.put_slice(value);
    }

    pub fn add_string(&mut self, value_tag: u8, name: &str, val: &str) {
        self.add_attribute(value_tag, name, val.as_bytes());
    }

    pub fn add_sub_string(&mut self, value_tag: u8, val: &str) {
        self.add_sub_attribute(value_tag, val.as_bytes());
    }

    pub fn add_integer(&mut self, name: &str, val: i32) {
        self.add_attribute(TAG_INTEGER, name, &val.to_be_bytes());
    }

    pub fn add_enum(&mut self, name: &str, val: i32) {
        self.add_attribute(TAG_ENUM, name, &val.to_be_bytes());
    }

    pub fn add_sub_enum(&mut self, val: i32) {
        self.add_sub_attribute(TAG_ENUM, &val.to_be_bytes());
    }

    pub fn add_boolean(&mut self, name: &str, val: bool) {
        self.add_attribute(TAG_BOOLEAN, name, &[if val { 1 } else { 0 }]);
    }

    pub fn finish(mut self) -> Bytes {
        self.buf.put_u8(TAG_END_OF_ATTRIBUTES);
        self.buf.freeze()
    }
}

/// Build standard IPP response for Get-Printer-Attributes
pub fn build_printer_attributes_response(
    req: &IppRequest,
    printer_name: &str,
    printer_uri: &str,
) -> Bytes {
    let mut resp = IppResponseBuilder::new(
        req.version_major,
        req.version_minor,
        STATUS_OK,
        req.request_id,
    );

    resp.start_group(TAG_PRINTER_ATTRIBUTES);
    resp.add_string(TAG_URI, "printer-uri-supported", printer_uri);
    resp.add_string(TAG_NAME_WITHOUT_LANG, "printer-name", printer_name);
    resp.add_string(
        TAG_TEXT_WITHOUT_LANG,
        "printer-info",
        &format!("Wireless Bridge: {}", printer_name),
    );
    resp.add_string(
        TAG_TEXT_WITHOUT_LANG,
        "printer-make-and-model",
        "Generic Driverless PDF Printer",
    );

    // State 3 = Idle / Ready
    resp.add_enum("printer-state", 3);
    resp.add_string(TAG_KEYWORD, "printer-state-reasons", "none");
    resp.add_boolean("printer-is-accepting-jobs", true);

    // Operations supported
    resp.add_enum("operations-supported", OP_PRINT_JOB as i32);
    resp.add_sub_enum(OP_VALIDATE_JOB as i32);
    resp.add_sub_enum(OP_GET_PRINTER_ATTRIBUTES as i32);
    resp.add_sub_enum(OP_GET_JOBS as i32);
    resp.add_sub_enum(OP_GET_JOB_ATTRIBUTES as i32);

    // Document formats supported (PDF and images)
    resp.add_string(TAG_MIME_TYPE, "document-format-supported", "application/pdf");
    resp.add_sub_string(TAG_MIME_TYPE, "application/octet-stream");
    resp.add_sub_string(TAG_MIME_TYPE, "image/pwg-raster");
    resp.add_sub_string(TAG_MIME_TYPE, "image/urf");
    resp.add_sub_string(TAG_MIME_TYPE, "image/jpeg");
    resp.add_sub_string(TAG_MIME_TYPE, "image/png");

    resp.add_string(TAG_KEYWORD, "ipp-versions-supported", "1.1");
    resp.add_sub_string(TAG_KEYWORD, "2.0");

    resp.add_string(TAG_KEYWORD, "pdl-override-supported", "attempted");
    resp.add_integer("queued-job-count", 0);

    resp.finish()
}

/// Build standard IPP response for Validate-Job
pub fn build_validate_job_response(req: &IppRequest) -> Bytes {
    let resp = IppResponseBuilder::new(
        req.version_major,
        req.version_minor,
        STATUS_OK,
        req.request_id,
    );
    resp.finish()
}

/// Build standard IPP response for Print-Job
pub fn build_print_job_response(req: &IppRequest, job_id: i32) -> Bytes {
    let mut resp = IppResponseBuilder::new(
        req.version_major,
        req.version_minor,
        STATUS_OK,
        req.request_id,
    );

    resp.start_group(TAG_JOB_ATTRIBUTES);
    resp.add_integer("job-id", job_id);
    // Job State 9 = Completed, 7 = Pending, 5 = Processing
    resp.add_enum("job-state", 7);
    resp.add_string(TAG_KEYWORD, "job-state-reasons", "job-incoming");

    resp.finish()
}
