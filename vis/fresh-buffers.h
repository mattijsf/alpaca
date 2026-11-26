/* SPDX-License-Identifier: LGPL-2.1-or-later */

/*
 * Included ahead of every C++ source of libprojectM by build-projectm.sh.
 *
 * projectM refills its vertex buffers every frame with glBufferSubData. On macOS, OpenGL runs
 * on Metal, and writing into a buffer the GPU may still read from makes the driver submit the
 * frame's work so far and start a new command buffer: several times a frame. Giving the
 * buffer new storage first (glBufferData with no data, the usual "orphaning") lets the driver
 * hand out fresh memory instead. Every caller in projectM draws only what it just wrote, so
 * nothing written before is lost.
 */

#ifndef ALPACA_FRESH_BUFFERS_H
#define ALPACA_FRESH_BUFFERS_H

#ifndef GL_SILENCE_DEPRECATION
#define GL_SILENCE_DEPRECATION
#endif
#include <OpenGL/gl3.h>

static inline void alpaca_buffer_sub_data(GLenum target, GLintptr offset, GLsizeiptr size, const void *data)
{
    GLint whole = 0;
    glGetBufferParameteriv(target, GL_BUFFER_SIZE, &whole);
    if (offset == 0 && size > 0 && size <= whole) {
        GLint usage = GL_STREAM_DRAW;
        glGetBufferParameteriv(target, GL_BUFFER_USAGE, &usage);
        glBufferData(target, whole, 0, (GLenum)usage);
    }
    glBufferSubData(target, offset, size, data);
}

#define glBufferSubData alpaca_buffer_sub_data

#endif
