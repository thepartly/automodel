-- Comprehensive test table with all PostgreSQL types
CREATE TABLE IF NOT EXISTS public.all_types_test (
    id SERIAL PRIMARY KEY,
    -- Boolean & Numeric Types
    bool_col BOOLEAN,
    char_col CHAR,
    int2_col SMALLINT,
    int4_col INTEGER,
    int8_col BIGINT,
    float4_col REAL,
    float8_col DOUBLE PRECISION,
    numeric_col NUMERIC(10, 2),
    -- String & Text Types
    name_col NAME,
    text_col TEXT,
    varchar_col VARCHAR(100),
    bpchar_col CHAR(10),
    -- Binary & Bit Types
    bytea_col BYTEA,
    bit_col BIT(8),
    varbit_col VARBIT(16),
    -- Date & Time Types
    date_col DATE,
    time_col TIME,
    timestamp_col TIMESTAMP,
    timestamptz_col TIMESTAMPTZ,
    interval_col INTERVAL,
    timetz_col TIMETZ,
    -- Range Types
    int4_range_col INT4RANGE,
    int8_range_col INT8RANGE,
    num_range_col NUMRANGE,
    ts_range_col TSRANGE,
    tstz_range_col TSTZRANGE,
    date_range_col DATERANGE,
    -- Multirange Types
    int4_multirange_col INT4MULTIRANGE,
    int8_multirange_col INT8MULTIRANGE,
    num_multirange_col NUMMULTIRANGE,
    ts_multirange_col TSMULTIRANGE,
    tstz_multirange_col TSTZMULTIRANGE,
    date_multirange_col DATEMULTIRANGE,
    -- Network & Address Types
    inet_col INET,
    cidr_col CIDR,
    macaddr_col MACADDR,
    -- JSON Types
    json_col JSON,
    jsonb_col JSONB,
    -- UUID Type
    uuid_col UUID,
    -- Array Types
    bool_array_col BOOLEAN [],
    int4_array_col INTEGER [],
    int8_array_col BIGINT [],
    text_array_col TEXT [],
    float8_array_col DOUBLE PRECISION [],
    -- Range Array Types
    int4_range_array_col INT4RANGE [],
    date_range_array_col DATERANGE [],
    -- Multirange Array Types
    int4_multirange_array_col INT4MULTIRANGE [],
    date_multirange_array_col DATEMULTIRANGE [],
    created_at TIMESTAMPTZ DEFAULT NOW()
);
-- Insert a sample row with all types
INSERT INTO public.all_types_test (
        bool_col,
        char_col,
        int2_col,
        int4_col,
        int8_col,
        float4_col,
        float8_col,
        numeric_col,
        name_col,
        text_col,
        varchar_col,
        bpchar_col,
        bytea_col,
        bit_col,
        varbit_col,
        date_col,
        time_col,
        timestamp_col,
        timestamptz_col,
        interval_col,
        timetz_col,
        int4_range_col,
        int8_range_col,
        num_range_col,
        ts_range_col,
        tstz_range_col,
        date_range_col,
        int4_multirange_col,
        int8_multirange_col,
        num_multirange_col,
        ts_multirange_col,
        tstz_multirange_col,
        date_multirange_col,
        inet_col,
        cidr_col,
        macaddr_col,
        json_col,
        jsonb_col,
        uuid_col,
        bool_array_col,
        int4_array_col,
        int8_array_col,
        text_array_col,
        float8_array_col,
        int4_range_array_col,
        date_range_array_col,
        int4_multirange_array_col,
        date_multirange_array_col
    )
VALUES (
        true,
        -- bool_col
        'A',
        -- char_col
        32767,
        -- int2_col
        2147483647,
        -- int4_col
        9223372036854775807,
        -- int8_col
        3.14159::real,
        -- float4_col
        2.718281828459045,
        -- float8_col
        12345.67,
        -- numeric_col
        'test_name',
        -- name_col
        'This is a test text',
        -- text_col
        'varchar test',
        -- varchar_col
        'bpchar',
        -- bpchar_col
        '\xDEADBEEF'::bytea,
        -- bytea_col
        B'10101010',
        -- bit_col
        B'1100110011001100',
        -- varbit_col
        '2025-11-20',
        -- date_col
        '14:30:00',
        -- time_col
        '2025-11-20 14:30:00',
        -- timestamp_col
        '2025-11-20 14:30:00+00',
        -- timestamptz_col
        '1 day 2 hours 30 minutes',
        -- interval_col
        '14:30:00+00',
        -- timetz_col
        '[1,10)',
        -- int4_range_col
        '[100,200]',
        -- int8_range_col
        '[0.5,99.9]',
        -- num_range_col
        '["2025-01-01 00:00:00","2025-12-31 23:59:59"]',
        -- ts_range_col
        '["2025-01-01 00:00:00+00","2025-12-31 23:59:59+00"]',
        -- tstz_range_col
        '[2025-01-01,2025-12-31]',
        -- date_range_col
        '{[1,5),[10,15)}',
        -- int4_multirange_col
        '{[100,200),[300,400)}',
        -- int8_multirange_col
        '{[1.5,5.5),[10.5,15.5)}',
        -- num_multirange_col
        '{[2025-01-01 00:00:00,2025-01-31 23:59:59],[2025-06-01 00:00:00,2025-06-30 23:59:59]}',
        -- ts_multirange_col
        '{[2025-01-01 00:00:00+00,2025-01-31 23:59:59+00],[2025-06-01 00:00:00+00,2025-06-30 23:59:59+00]}',
        -- tstz_multirange_col
        '{[2025-01-01,2025-01-31],[2025-06-01,2025-06-30]}',
        -- date_multirange_col
        '192.168.1.1',
        -- inet_col
        '192.168.1.0/24',
        -- cidr_col
        '08:00:2b:01:02:03',
        -- macaddr_col
        '{"key": "value", "number": 42}',
        -- json_col
        '{"name": "test", "tags": ["tag1", "tag2"]}',
        -- jsonb_col
        'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11',
        -- uuid_col
        ARRAY [true, false, true],
        -- bool_array_col
        ARRAY [1, 2, 3, 4, 5],
        -- int4_array_col
        ARRAY [100::bigint, 200::bigint, 300::bigint],
        -- int8_array_col
        ARRAY ['one', 'two', 'three'],
        -- text_array_col
        ARRAY [1.1, 2.2, 3.3],
        -- float8_array_col
        ARRAY ['[1,5)'::int4range, '[10,20)'::int4range],
        -- int4_range_array_col
        ARRAY ['[2025-01-01,2025-01-31] '::daterange, ' [2025-06-01,2025-06-30] '::daterange], -- date_range_array_col
    ARRAY[' { [1,5),[10,15)}'::int4multirange, '{[20,25),[30,35)}'::int4multirange],
        -- int4_multirange_array_col
        ARRAY ['{[2025-01-01,2025-01-15],
        [2025-01-20,2025-01-31] } '::datemultirange, ' { [2025-06-01,2025-06-15] } '::datemultirange] -- date_multirange_array_col
) ON CONFLICT DO NOTHING;
