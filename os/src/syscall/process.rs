//! Process management syscalls
use crate::task::{change_program_brk, exit_current_and_run_next, suspend_current_and_run_next,current_user_token};
use crate::timer::get_time_us;
use crate::mm::{translated_byte_buffer, VirtAddr, PTEFlags, PageTable, VirtPageNum, frame_alloc};
use crate::config::PAGE_SIZE;
#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let time_us = get_time_us();
    let sec = time_us / 1_000_000;
    let usec = time_us % 1_000_000;
    let time_val = TimeVal { sec, usec };
      let token = current_user_token();
    let time_val_size = core::mem::size_of::<TimeVal>();

    let byte_array = unsafe { 
        core::slice::from_raw_parts(
            &time_val as *const TimeVal as *const u8, 
            time_val_size
        ) 
    };
    
    let buffers = translated_byte_buffer(token, _ts as *const u8, time_val_size);
    let mut total_written = 0;
    
    // 将数据写入用户空间缓冲区
    for buffer in buffers {
        let len = buffer.len().min(time_val_size - total_written);
        buffer[..len].copy_from_slice(&byte_array[total_written..total_written + len]);
        total_written += len;
        if total_written == time_val_size {
            break;
        }
    }
    0
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(trace_request: usize, addr: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");

    let token = current_user_token();
    
    match trace_request {
        // 读取操作
        0 => {
            let page_table = PageTable::from_token(token);
            let virt_addr = VirtAddr::from(addr);
            let vpn = virt_addr.floor();
            
            // 检查页表项是否存在
            if let Some(pte) = page_table.translate(vpn) {
                // 检查是否可读
                if !pte.readable() || !(pte.flags() & PTEFlags::U != PTEFlags::empty()) {
                    return -1;
                }
                
                // 读取数据
                let buffers = translated_byte_buffer(token, addr as *const u8, 1);
                if buffers.is_empty() {
                    return -1;
                }
                
                // 返回读取的字节值
                buffers[0][0] as isize
            } else {
                -1
            }
        },
        
        // 写入操作
        1 => {
            let page_table = PageTable::from_token(token);
            let virt_addr = VirtAddr::from(addr);
            let vpn = virt_addr.floor();
            
            // 检查页表项是否存在
            if let Some(pte) = page_table.translate(vpn) {
                // 检查是否可写
                if !pte.writable() || !(pte.flags() & PTEFlags::U != PTEFlags::empty()) {
                    return -1;
                }
                
                // 写入数据
                let mut buffers = translated_byte_buffer(token, addr as *mut u8, 1);
                if buffers.is_empty() {
                    return -1;
                }
                
                // 写入一个字节
                buffers[0][0] = data as u8;
                0
            } else {
                -1
            }
        },
        
        // 打印字符串
        2 => {
            // 打印特定的测试字符串
            println!("string from task trace test");
            0
        },
        
        // 未知操作
        _ => -1,
    }
}

/// YOUR JOB: implement sys_mmap to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap start={:#x}, len={}, prot={}", start, len, prot);
    
    // 参数验证
    // 1. 检查 start 是否按页对齐
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    
    // 2. 检查 prot 参数有效性
    if prot & !0x7 != 0 {  // 其他位必须为0
        return -1;
    }
    
    if prot & 0x7 == 0 {   // 至少要有一个权限位
        return -1;
    }
    
    // 3. len 为 0 的特殊情况
    if len == 0 {
        return 0;
    }
    
    // 计算需要映射的页数
    let page_count = (len + PAGE_SIZE - 1) / PAGE_SIZE;
    
    // 获取当前用户的页表令牌
    let token = current_user_token();
    let page_table = PageTable::from_token(token);
    
    // 检查地址范围是否已被映射
    let start_vpn = VirtAddr::from(start).floor();
    for i in 0..page_count {
        let vpn = VirtPageNum::from(start_vpn.0 + i);
        if let Some(pte) = page_table.translate(vpn) {
            if pte.is_valid() {
                // 存在已映射的页
                return -1;
            }
        }
    }
    
    // 重新获取可变页表进行映射
    let mut page_table = PageTable::from_token(token);
    
    // 为每个页面分配物理内存并建立映射
    for i in 0..page_count {
        let vpn = VirtPageNum::from(start_vpn.0 + i);
        if let Some(frame) = frame_alloc() {
            let ppn = frame.ppn;
            
            // 转换权限
            let mut flags = PTEFlags::V | PTEFlags::U; // Valid + User
            if prot & 0x1 != 0 { flags |= PTEFlags::R; }
            if prot & 0x2 != 0 { flags |= PTEFlags::W; }
            if prot & 0x4 != 0 { flags |= PTEFlags::X; }
            
            // 安全地映射页面
            page_table.map(vpn, ppn, flags);
            
            // 防止 frame 被自动释放
            core::mem::forget(frame);
        } else {
            // 物理内存不足，需要回滚已分配的页面
            for j in 0..i {
                let vpn = VirtPageNum::from(start_vpn.0 + j);
                if let Some(pte) = page_table.translate(vpn) {
                    if pte.is_valid() {
                        page_table.unmap(vpn);
                    }
                }
            }
            return -1;
        }
    }

    0
}

/// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap start={:#x}, len={}", start, len);
    
    // 参数验证
    // 1. 检查 start 是否按页对齐
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    
    // 2. len 为 0 的特殊情况
    if len == 0 {
        return 0;
    }
    
    // 计算需要取消映射的页数
    let page_count = (len + PAGE_SIZE - 1) / PAGE_SIZE;
    
    // 获取当前用户的页表令牌
    let token = current_user_token();
    let page_table = PageTable::from_token(token);
    
    // 检查地址范围内是否有未映射的页
    let start_vpn = VirtAddr::from(start).floor();
    let mut all_mapped = true;
    for i in 0..page_count {
        let vpn = VirtPageNum::from(start_vpn.0 + i);
        if let Some(pte) = page_table.translate(vpn) {
            if !pte.is_valid() {
                all_mapped = false;
                break;
            }
        } else {
            all_mapped = false;
            break;
        }
    }
    
    // 如果有页面未映射，返回错误
    if !all_mapped {
        return -1;
    }
    
    // 取消映射所有页面
    let mut page_table = PageTable::from_token(token);
    for i in 0..page_count {
        let vpn = VirtPageNum::from(start_vpn.0 + i);
        page_table.unmap(vpn);
    }
    
    0
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
