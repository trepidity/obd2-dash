"""
Ircama ELM327-emulator scenario for a 2020 Chevrolet Malibu 1.5T (LT trim).
VIN: 1G1ZD5ST2LF000000

Usage:
    echo -e 'merge malibu_2020\nscenario malibu_2020' | python3 -m elm -b /tmp/emu.txt

All PID response values represent realistic warm-idle conditions for the
1.5L turbocharged 4-cylinder (LFV/L3A engine family).
"""

from elm.obd_message import ELM_FOOTER, ST

# ── Hex encoding helpers ─────────────────────────────────────────────────────

def _encode_1byte_pct(pid_code, value):
    """A * 100 / 255  →  A = value * 255 / 100"""
    a = int(round(value * 255.0 / 100.0))
    a = max(0, min(255, a))
    return ST(f'41 {pid_code:02X} {a:02X}')

def _encode_1byte_temp(pid_code, temp_c):
    """A - 40  →  A = temp + 40"""
    a = int(round(temp_c + 40.0))
    a = max(0, min(255, a))
    return ST(f'41 {pid_code:02X} {a:02X}')

def _encode_1byte_direct(pid_code, value):
    """A (direct)"""
    a = int(round(value))
    a = max(0, min(255, a))
    return ST(f'41 {pid_code:02X} {a:02X}')

def _encode_1byte_fuel_trim(pid_code, pct):
    """(A - 128) * 100 / 128  →  A = pct * 128 / 100 + 128"""
    a = int(round(pct * 128.0 / 100.0 + 128.0))
    a = max(0, min(255, a))
    return ST(f'41 {pid_code:02X} {a:02X}')

def _encode_2byte_rpm(pid_code, rpm):
    """(256A + B) / 4  →  raw = rpm * 4"""
    raw = int(round(rpm * 4.0))
    raw = max(0, min(65535, raw))
    return ST(f'41 {pid_code:02X} {(raw >> 8) & 0xFF:02X} {raw & 0xFF:02X}')

def _encode_2byte_div100(pid_code, value):
    """(256A + B) / 100"""
    raw = int(round(value * 100.0))
    raw = max(0, min(65535, raw))
    return ST(f'41 {pid_code:02X} {(raw >> 8) & 0xFF:02X} {raw & 0xFF:02X}')

def _encode_2byte_div10_minus40(pid_code, temp_c):
    """(256A + B) / 10 - 40  →  raw = (temp + 40) * 10"""
    raw = int(round((temp_c + 40.0) * 10.0))
    raw = max(0, min(65535, raw))
    return ST(f'41 {pid_code:02X} {(raw >> 8) & 0xFF:02X} {raw & 0xFF:02X}')

def _encode_2byte_div1000(pid_code, volts):
    """(256A + B) / 1000"""
    raw = int(round(volts * 1000.0))
    raw = max(0, min(65535, raw))
    return ST(f'41 {pid_code:02X} {(raw >> 8) & 0xFF:02X} {raw & 0xFF:02X}')

def _encode_2byte_direct(pid_code, value):
    """256A + B"""
    raw = int(round(value))
    raw = max(0, min(65535, raw))
    return ST(f'41 {pid_code:02X} {(raw >> 8) & 0xFF:02X} {raw & 0xFF:02X}')

def _encode_2byte_times10(pid_code, value):
    """(256A + B) * 10  →  raw = value / 10"""
    raw = int(round(value / 10.0))
    raw = max(0, min(65535, raw))
    return ST(f'41 {pid_code:02X} {(raw >> 8) & 0xFF:02X} {raw & 0xFF:02X}')

def _encode_2byte_div255_pct(pid_code, value):
    """(256A + B) * 100 / 255"""
    raw = int(round(value * 255.0 / 100.0))
    raw = max(0, min(65535, raw))
    return ST(f'41 {pid_code:02X} {(raw >> 8) & 0xFF:02X} {raw & 0xFF:02X}')

def _encode_2byte_div32768(pid_code, value):
    """(256A + B) / 32768"""
    raw = int(round(value * 32768.0))
    raw = max(0, min(65535, raw))
    return ST(f'41 {pid_code:02X} {(raw >> 8) & 0xFF:02X} {raw & 0xFF:02X}')

def _encode_2byte_div20(pid_code, value):
    """(256A + B) / 20"""
    raw = int(round(value * 20.0))
    raw = max(0, min(65535, raw))
    return ST(f'41 {pid_code:02X} {(raw >> 8) & 0xFF:02X} {raw & 0xFF:02X}')

def _encode_2byte_div4(pid_code, value):
    """(256A + B) / 4"""
    raw = int(round(value * 4.0))
    raw = max(0, min(65535, raw))
    return ST(f'41 {pid_code:02X} {(raw >> 8) & 0xFF:02X} {raw & 0xFF:02X}')

def _encode_1byte_timing(pid_code, degrees):
    """A/2 - 64  →  A = (degrees + 64) * 2"""
    a = int(round((degrees + 64.0) * 2.0))
    a = max(0, min(255, a))
    return ST(f'41 {pid_code:02X} {a:02X}')

def _encode_1byte_torque(pid_code, pct):
    """A - 125"""
    a = int(round(pct + 125.0))
    a = max(0, min(255, a))
    return ST(f'41 {pid_code:02X} {a:02X}')

def _encode_1byte_fuel_pressure(pid_code, kpa):
    """A * 3  →  A = kpa / 3"""
    a = int(round(kpa / 3.0))
    a = max(0, min(255, a))
    return ST(f'41 {pid_code:02X} {a:02X}')


# ── VIN: 1G1ZD5ST2LF000000 as CAN ISO-TP multi-frame ────────────────────────
# The app's read_vin() parser expects lines with "N:" prefixes:
#   0: 49 02 01 <data>   (header frame)
#   1: <data>            (continuation)
#   2: <data>            (continuation)

_VIN = '1G1ZD5ST2LF000000'
_vin_bytes = [ord(c) for c in _VIN]  # 17 bytes
# Frame 0: 49 02 01 + first 4 VIN bytes
_f0 = '49 02 01 ' + ' '.join(f'{b:02X}' for b in _vin_bytes[0:4])
# Frame 1: next 7 VIN bytes
_f1 = ' '.join(f'{b:02X}' for b in _vin_bytes[4:11])
# Frame 2: last 6 VIN bytes
_f2 = ' '.join(f'{b:02X}' for b in _vin_bytes[11:17])

_VIN_RESPONSE = (
    ST('0: ' + _f0) + ' '
    + ST('1: ' + _f1) + ' '
    + ST('2: ' + _f2)
)

# ── DTC bytes: P0171 + P0420 ─────────────────────────────────────────────────
# P0171: category=Powertrain(00), second=0(00), third=1(0001), fourth=7(0111), fifth=1(0001)
#   byte A = 00_00_0001 = 0x01, byte B = 0111_0001 = 0x71
# P0420: A = 00_00_0100 = 0x04, B = 0010_0000 = 0x20
_DTC_RESPONSE = ST('43 01 71 04 20')


# ── Supported PIDs bitmap for 0100 ───────────────────────────────────────────
# Standard PIDs 01-20 that the Malibu supports:
#   0x04 EngineLoad, 0x05 CoolantTemp, 0x06 STFT1, 0x07 LTFT1,
#   0x08 STFT2, 0x09 LTFT2, 0x0A FuelPressure, 0x0B IntakeMAP,
#   0x0C RPM, 0x0D Speed, 0x0E TimingAdv, 0x0F IntakeAirTemp,
#   0x10 MAF, 0x11 ThrottlePos, 0x1F RunTime
# Bitmap: positions 04-11 = 0xFF, 0E-11 = 0xF0 in byte 2, 1F = bit 2 in byte 4
# Computed: BE 3F A8 13
_SUPPORTED_PIDS = ST('41 00 BE 3F A8 13')


ObdMessage = {
    'malibu_2020': {

        # ── Supported PIDs ────────────────────────────────────────────────
        'PIDS_SUPPORTED_0100': {
            'Request': '^0100' + ELM_FOOTER,
            'Descr': 'PIDs supported [01-20]',
            'Response': _SUPPORTED_PIDS,
        },

        # ── VIN (Mode 09 PID 02) ─────────────────────────────────────────
        'VIN_0902': {
            'Request': '^0902',
            'Descr': 'Vehicle Identification Number',
            'Response': _VIN_RESPONSE,
        },

        # ── DTCs (Mode 03) ───────────────────────────────────────────────
        'DTCS_03': {
            'Request': '^03' + ELM_FOOTER,
            'Descr': 'Request stored DTCs',
            'Response': _DTC_RESPONSE,
        },

        # ── Battery voltage ──────────────────────────────────────────────
        'VOLTAGE_ATRV': {
            'Request': '^ATRV$',
            'Descr': 'Read voltage',
            'Response': ST('12.6V'),
        },

        # ── Engine / Drivetrain PIDs ─────────────────────────────────────

        # 0x04 Engine Load: ~20%
        'PID_04_ENGINE_LOAD': {
            'Request': '^0104' + ELM_FOOTER,
            'Descr': 'Calculated Engine Load',
            'Response': _encode_1byte_pct(0x04, 20.0),
        },
        # 0x05 Coolant Temp: 90 C
        'PID_05_COOLANT_TEMP': {
            'Request': '^0105' + ELM_FOOTER,
            'Descr': 'Engine Coolant Temperature',
            'Response': _encode_1byte_temp(0x05, 90.0),
        },
        # 0x06 Short Fuel Trim Bank 1: +2.3%
        'PID_06_STFT_B1': {
            'Request': '^0106' + ELM_FOOTER,
            'Descr': 'Short Term Fuel Trim Bank 1',
            'Response': _encode_1byte_fuel_trim(0x06, 2.3),
        },
        # 0x07 Long Fuel Trim Bank 1: -1.5%
        'PID_07_LTFT_B1': {
            'Request': '^0107' + ELM_FOOTER,
            'Descr': 'Long Term Fuel Trim Bank 1',
            'Response': _encode_1byte_fuel_trim(0x07, -1.5),
        },
        # 0x08 Short Fuel Trim Bank 2: +1.0%
        'PID_08_STFT_B2': {
            'Request': '^0108' + ELM_FOOTER,
            'Descr': 'Short Term Fuel Trim Bank 2',
            'Response': _encode_1byte_fuel_trim(0x08, 1.0),
        },
        # 0x09 Long Fuel Trim Bank 2: -0.5%
        'PID_09_LTFT_B2': {
            'Request': '^0109' + ELM_FOOTER,
            'Descr': 'Long Term Fuel Trim Bank 2',
            'Response': _encode_1byte_fuel_trim(0x09, -0.5),
        },
        # 0x0A Fuel Pressure: 300 kPa
        'PID_0A_FUEL_PRESSURE': {
            'Request': '^010A' + ELM_FOOTER,
            'Descr': 'Fuel Pressure',
            'Response': _encode_1byte_fuel_pressure(0x0A, 300.0),
        },
        # 0x0B Intake MAP: 33 kPa (idle vacuum)
        'PID_0B_INTAKE_MAP': {
            'Request': '^010B' + ELM_FOOTER,
            'Descr': 'Intake Manifold Absolute Pressure',
            'Response': _encode_1byte_direct(0x0B, 33.0),
        },
        # 0x0C Engine RPM: 800 rpm (idle)
        'PID_0C_RPM': {
            'Request': '^010C' + ELM_FOOTER,
            'Descr': 'Engine RPM',
            'Response': _encode_2byte_rpm(0x0C, 800.0),
        },
        # 0x0D Vehicle Speed: 0 km/h
        'PID_0D_SPEED': {
            'Request': '^010D' + ELM_FOOTER,
            'Descr': 'Vehicle Speed',
            'Response': _encode_1byte_direct(0x0D, 0.0),
        },
        # 0x0E Timing Advance: 12 degrees
        'PID_0E_TIMING_ADVANCE': {
            'Request': '^010E' + ELM_FOOTER,
            'Descr': 'Timing Advance',
            'Response': _encode_1byte_timing(0x0E, 12.0),
        },
        # 0x0F Intake Air Temp: 35 C
        'PID_0F_INTAKE_AIR_TEMP': {
            'Request': '^010F' + ELM_FOOTER,
            'Descr': 'Intake Air Temperature',
            'Response': _encode_1byte_temp(0x0F, 35.0),
        },
        # 0x10 MAF: 3.5 g/s (idle)
        'PID_10_MAF': {
            'Request': '^0110' + ELM_FOOTER,
            'Descr': 'MAF Air Flow Rate',
            'Response': _encode_2byte_div100(0x10, 3.5),
        },
        # 0x11 Throttle Position: 15%
        'PID_11_THROTTLE': {
            'Request': '^0111' + ELM_FOOTER,
            'Descr': 'Throttle Position',
            'Response': _encode_1byte_pct(0x11, 15.0),
        },

        # ── Extended PIDs ────────────────────────────────────────────────

        # 0x1F Run Time Since Start: 120 seconds
        'PID_1F_RUN_TIME': {
            'Request': '^011F' + ELM_FOOTER,
            'Descr': 'Run Time Since Engine Start',
            'Response': _encode_2byte_direct(0x1F, 120.0),
        },
        # 0x21 Distance with MIL: 50 km
        'PID_21_DIST_MIL': {
            'Request': '^0121' + ELM_FOOTER,
            'Descr': 'Distance Traveled with MIL On',
            'Response': _encode_2byte_direct(0x21, 50.0),
        },
        # 0x23 Fuel Rail Gauge Pressure: 35000 kPa
        'PID_23_FUEL_RAIL_GAUGE': {
            'Request': '^0123' + ELM_FOOTER,
            'Descr': 'Fuel Rail Gauge Pressure',
            'Response': _encode_2byte_times10(0x23, 35000.0),
        },
        # 0x2C Commanded EGR: 5%
        'PID_2C_EGR': {
            'Request': '^012C' + ELM_FOOTER,
            'Descr': 'Commanded EGR',
            'Response': _encode_1byte_pct(0x2C, 5.0),
        },
        # 0x2E Commanded Evap Purge: 3%
        'PID_2E_EVAP_PURGE': {
            'Request': '^012E' + ELM_FOOTER,
            'Descr': 'Commanded Evaporative Purge',
            'Response': _encode_1byte_pct(0x2E, 3.0),
        },
        # 0x2F Fuel Tank Level: 65%
        'PID_2F_FUEL_LEVEL': {
            'Request': '^012F' + ELM_FOOTER,
            'Descr': 'Fuel Tank Level Input',
            'Response': _encode_1byte_pct(0x2F, 65.0),
        },
        # 0x31 Distance Since DTC Clear: 1200 km
        'PID_31_DIST_DTC_CLR': {
            'Request': '^0131' + ELM_FOOTER,
            'Descr': 'Distance Since DTC Cleared',
            'Response': _encode_2byte_direct(0x31, 1200.0),
        },
        # 0x33 Barometric Pressure: 101 kPa
        'PID_33_BARO': {
            'Request': '^0133' + ELM_FOOTER,
            'Descr': 'Barometric Pressure',
            'Response': _encode_1byte_direct(0x33, 101.0),
        },
        # 0x3C Catalyst Temp B1S1: 450 C
        'PID_3C_CAT_B1S1': {
            'Request': '^013C' + ELM_FOOTER,
            'Descr': 'Catalyst Temperature B1S1',
            'Response': _encode_2byte_div10_minus40(0x3C, 450.0),
        },
        # 0x3D Catalyst Temp B2S1: NO DATA (single-bank engine)
        'PID_3D_CAT_B2S1': {
            'Request': '^013D' + ELM_FOOTER,
            'Descr': 'Catalyst Temperature B2S1',
            'Response': ST('NO DATA'),
        },
        # 0x3E Catalyst Temp B1S2: 420 C
        'PID_3E_CAT_B1S2': {
            'Request': '^013E' + ELM_FOOTER,
            'Descr': 'Catalyst Temperature B1S2',
            'Response': _encode_2byte_div10_minus40(0x3E, 420.0),
        },
        # 0x3F Catalyst Temp B2S2: NO DATA
        'PID_3F_CAT_B2S2': {
            'Request': '^013F' + ELM_FOOTER,
            'Descr': 'Catalyst Temperature B2S2',
            'Response': ST('NO DATA'),
        },
        # 0x42 Control Module Voltage: 14.2 V
        'PID_42_VOLTAGE': {
            'Request': '^0142' + ELM_FOOTER,
            'Descr': 'Control Module Voltage',
            'Response': _encode_2byte_div1000(0x42, 14.2),
        },
        # 0x43 Absolute Load: 18%
        'PID_43_ABS_LOAD': {
            'Request': '^0143' + ELM_FOOTER,
            'Descr': 'Absolute Load Value',
            'Response': _encode_2byte_div255_pct(0x43, 18.0),
        },
        # 0x44 Commanded Equivalence Ratio: 1.0 (stoich)
        'PID_44_EQUIV_RATIO': {
            'Request': '^0144' + ELM_FOOTER,
            'Descr': 'Commanded Equivalence Ratio',
            'Response': _encode_2byte_div32768(0x44, 1.0),
        },
        # 0x45 Relative Throttle Pos: 12%
        'PID_45_REL_THROTTLE': {
            'Request': '^0145' + ELM_FOOTER,
            'Descr': 'Relative Throttle Position',
            'Response': _encode_1byte_pct(0x45, 12.0),
        },
        # 0x46 Ambient Air Temp: 22 C
        'PID_46_AMBIENT_TEMP': {
            'Request': '^0146' + ELM_FOOTER,
            'Descr': 'Ambient Air Temperature',
            'Response': _encode_1byte_temp(0x46, 22.0),
        },
        # 0x47 Absolute Throttle Pos B: 16%
        'PID_47_ABS_THROTTLE_B': {
            'Request': '^0147' + ELM_FOOTER,
            'Descr': 'Absolute Throttle Position B',
            'Response': _encode_1byte_pct(0x47, 16.0),
        },
        # 0x49 Accel Pedal Pos D: 14%
        'PID_49_ACCEL_D': {
            'Request': '^0149' + ELM_FOOTER,
            'Descr': 'Accelerator Pedal Position D',
            'Response': _encode_1byte_pct(0x49, 14.0),
        },
        # 0x4A Accel Pedal Pos E: 14%
        'PID_4A_ACCEL_E': {
            'Request': '^014A' + ELM_FOOTER,
            'Descr': 'Accelerator Pedal Position E',
            'Response': _encode_1byte_pct(0x4A, 14.0),
        },
        # 0x4C Commanded Throttle Actuator: 15%
        'PID_4C_CMD_THROTTLE': {
            'Request': '^014C' + ELM_FOOTER,
            'Descr': 'Commanded Throttle Actuator',
            'Response': _encode_1byte_pct(0x4C, 15.0),
        },
        # 0x59 Fuel Rail Abs Pressure: 35500 kPa
        'PID_59_FUEL_RAIL_ABS': {
            'Request': '^0159' + ELM_FOOTER,
            'Descr': 'Fuel Rail Absolute Pressure',
            'Response': _encode_2byte_times10(0x59, 35500.0),
        },
        # 0x5C Engine Oil Temp: 95 C
        'PID_5C_OIL_TEMP': {
            'Request': '^015C' + ELM_FOOTER,
            'Descr': 'Engine Oil Temperature',
            'Response': _encode_1byte_temp(0x5C, 95.0),
        },
        # 0x5E Engine Fuel Rate: 0.8 L/h (idle)
        'PID_5E_FUEL_RATE': {
            'Request': '^015E' + ELM_FOOTER,
            'Descr': 'Engine Fuel Rate',
            'Response': _encode_2byte_div20(0x5E, 0.8),
        },
        # 0x61 Demanded Torque: -2% (coasting at idle)
        'PID_61_DEMAND_TORQUE': {
            'Request': '^0161' + ELM_FOOTER,
            'Descr': 'Driver Demand Engine Torque',
            'Response': _encode_1byte_torque(0x61, -2.0),
        },
        # 0x62 Actual Torque: 15%
        'PID_62_ACTUAL_TORQUE': {
            'Request': '^0162' + ELM_FOOTER,
            'Descr': 'Actual Engine Torque',
            'Response': _encode_1byte_torque(0x62, 15.0),
        },
        # 0x63 Reference Torque: 250 Nm
        'PID_63_REF_TORQUE': {
            'Request': '^0163' + ELM_FOOTER,
            'Descr': 'Engine Reference Torque',
            'Response': _encode_2byte_direct(0x63, 250.0),
        },

        # ── Custom/Extended PIDs → NO DATA ───────────────────────────────
        # 0xFE Transmission Temp (custom): NO DATA
        'PID_FE_TRANS_TEMP': {
            'Request': '^01FE' + ELM_FOOTER,
            'Descr': 'Transmission Temperature (custom)',
            'Response': ST('NO DATA'),
        },
        # 0xFD Oil Pressure (custom): NO DATA
        'PID_FD_OIL_PRESSURE': {
            'Request': '^01FD' + ELM_FOOTER,
            'Descr': 'Oil Pressure (custom)',
            'Response': ST('NO DATA'),
        },
    },
}

# Set Priority 1 on all entries so they override the emulator's built-in
# 'default' scenario entries (which have implicit Priority 10).
for _entry in ObdMessage['malibu_2020'].values():
    _entry['Priority'] = 1
